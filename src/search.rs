//! Busca da v1: negamax alpha-beta fail-soft com aprofundamento iterativo, janelas de aspiração,
//! PVS, extensão de xeque, busca quiescente, tabela de transposição e ordenação TT → MVV-LVA.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use crate::eval::evaluate;
use crate::movegen::{generate_legal, generate_pseudo_legal};
use crate::moves::{MAX_MOVES, Move, MoveKind, MoveList};
use crate::nnue::{Accumulators, Network};
use crate::position::Position;
use crate::see::see;
use crate::timeman::{Limits, should_start_iteration};
use crate::tt::{Bound, TranspositionTable};
use crate::types::{Color, PieceType};

pub const MAX_PLY: usize = 128;
pub const INFINITY: i32 = 32_000;
pub const MATE: i32 = 31_000;
/// Pontuações além disso são mate em até `MAX_PLY` meios-lances.
pub const MATE_BOUND: i32 = MATE - MAX_PLY as i32;

const RFP_MAX_DEPTH: i32 = 8;
const RFP_MARGIN: i32 = 80;
const NMP_MIN_DEPTH: i32 = 3;
const LMR_MIN_DEPTH: i32 = 3;
const IIR_MIN_DEPTH: i32 = 4;
const LMP_MAX_DEPTH: i32 = 8;
const FUTILITY_MAX_DEPTH: i32 = 6;
const FUTILITY_BASE: i32 = 100;
const FUTILITY_MARGIN: i32 = 100;
const SEE_PRUNE_MAX_DEPTH: i32 = 8;
const SEE_QUIET_MARGIN: i32 = 50;
const SEE_CAPTURE_MARGIN: i32 = 100;

/// Quantos lances um nó de profundidade `depth` busca antes de o LMP podar os quietos restantes.
fn lmp_threshold(depth: i32, improving: bool) -> usize {
    let base = (3 + depth * depth) as usize;
    if improving { base } else { base / 2 }
}

/// Informação de uma iteração completa, para a linha `info` do UCI.
#[derive(Clone, Debug)]
pub struct IterationInfo {
    pub depth: u32,
    pub seldepth: u32,
    pub score: i32,
    pub nodes: u64,
    pub elapsed: Duration,
    pub hashfull: u32,
    pub pv: Vec<Move>,
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best_move: Option<Move>,
    pub score: i32,
    pub depth: u32,
    pub nodes: u64,
    pub pv: Vec<Move>,
}

/// Teto do histórico; a "gravidade" puxa os valores de volta para zero perto dele.
const HISTORY_MAX: i32 = 16_384;

/// Histórico de lances quietos ("butterfly"): quanto cada lance (cor, origem, destino) causou
/// cortes. Guia a ordenação e as reduções.
struct History {
    table: [[[i32; 64]; 64]; 2],
}

impl History {
    fn new() -> Box<History> {
        Box::new(History {
            table: [[[0; 64]; 64]; 2],
        })
    }

    fn get(&self, color: Color, mv: Move) -> i32 {
        self.table[color.index()][mv.from().index()][mv.to().index()]
    }

    /// Soma `bonus` (negativo para punir) com gravidade: nunca passa de `HISTORY_MAX`.
    fn update(&mut self, color: Color, mv: Move, bonus: i32) {
        let entry = &mut self.table[color.index()][mv.from().index()][mv.to().index()];
        let bonus = bonus.clamp(-HISTORY_MAX, HISTORY_MAX);
        *entry += bonus - *entry * bonus.abs() / HISTORY_MAX;
    }

    fn clear(&mut self) {
        self.table = [[[0; 64]; 64]; 2];
    }
}

/// Entradas por cor da tabela de correção (potência de 2).
const CORRECTION_SIZE: usize = 16_384;
/// As entradas guardam centipeões multiplicados por isto, para a média móvel não perder precisão.
const CORRECTION_GRAIN: i32 = 256;
/// Maior correção aplicada, em centipeões.
const CORRECTION_MAX: i32 = 100;

/// Correção da avaliação estática pela estrutura de peões: média móvel da diferença entre o que a
/// busca achou e o que a avaliação dizia, em posições com os mesmos peões. Conserta, aos poucos,
/// o que a avaliação erra de forma sistemática naquele tipo de posição.
struct CorrectionHistory {
    table: [[i32; CORRECTION_SIZE]; 2],
}

impl CorrectionHistory {
    fn new() -> Box<CorrectionHistory> {
        Box::new(CorrectionHistory {
            table: [[0; CORRECTION_SIZE]; 2],
        })
    }

    fn index(pawn_hash: u64) -> usize {
        pawn_hash as usize & (CORRECTION_SIZE - 1)
    }

    /// Correção, em centipeões, para o lado `color` a jogar.
    fn get(&self, color: Color, pawn_hash: u64) -> i32 {
        self.table[color.index()][Self::index(pawn_hash)] / CORRECTION_GRAIN
    }

    /// Puxa a entrada na direção de `error` (busca menos avaliação crua); buscas mais fundas
    /// pesam mais.
    fn update(&mut self, color: Color, pawn_hash: u64, error: i32, depth: i32) {
        let weight = (depth + 1).clamp(1, 16);
        let limit = CORRECTION_MAX * CORRECTION_GRAIN;
        let target = (error * CORRECTION_GRAIN).clamp(-limit, limit);
        let entry = &mut self.table[color.index()][Self::index(pawn_hash)];
        *entry += (target - *entry) * weight / 256;
    }

    fn clear(&mut self) {
        self.table = [[0; CORRECTION_SIZE]; 2];
    }
}

/// O resultado da busca num nó diz para que lado a avaliação estática errou? Exato sempre diz;
/// falha alta só quando passou da avaliação, falha baixa só quando ficou abaixo. Mate não conta.
fn correction_applies(bound: Bound, best_score: i32, static_eval: i32) -> bool {
    if best_score.abs() >= MATE_BOUND {
        return false;
    }
    match bound {
        Bound::Exact => true,
        Bound::Lower => best_score > static_eval,
        Bound::Upper => best_score < static_eval,
    }
}

/// Redução base do LMR para a profundidade e o número do lance (1 = primeiro lance legal).
fn lmr_reduction(depth: i32, move_number: usize) -> i32 {
    LMR_TABLE[(depth.max(0) as usize).min(63)][move_number.min(63)]
}

/// `0.75 + ln(profundidade)·ln(número do lance)/2.25`, arredondado para baixo.
static LMR_TABLE: LazyLock<[[i32; 64]; 64]> = LazyLock::new(|| {
    let mut table = [[0; 64]; 64];
    for (depth, row) in table.iter_mut().enumerate().skip(1) {
        for (number, cell) in row.iter_mut().enumerate().skip(1) {
            *cell = (0.75 + (depth as f64).ln() * (number as f64).ln() / 2.25) as i32;
        }
    }
    table
});

pub struct Searcher {
    tt: TranspositionTable,
    history: Box<History>,
    correction: Box<CorrectionHistory>,
    /// Rede neural da avaliação; sem ela, a avaliação à mão.
    network: Option<Arc<Network>>,
}

impl Searcher {
    pub fn new(hash_megabytes: usize) -> Searcher {
        Searcher {
            tt: TranspositionTable::new(hash_megabytes),
            history: History::new(),
            correction: CorrectionHistory::new(),
            network: None,
        }
    }

    /// Troca a avaliação: `Some` passa a usar a rede; `None` volta à avaliação à mão.
    pub fn set_network(&mut self, network: Option<Arc<Network>>) {
        self.network = network;
    }

    /// A rede em uso, se houver.
    pub fn network(&self) -> Option<&Network> {
        self.network.as_deref()
    }

    pub fn resize(&mut self, hash_megabytes: usize) {
        self.tt = TranspositionTable::new(hash_megabytes);
    }

    pub fn clear(&mut self) {
        self.tt.clear();
        self.history.clear();
        self.correction.clear();
    }

    /// Busca a partir de `root`. `history` traz os hashes das posições anteriores da partida (sem
    /// a raiz), para reconhecer repetições. A primeira iteração sempre termina, mesmo com `stop`
    /// ligado, para que sempre haja um lance a devolver.
    pub fn search(
        &mut self,
        root: &Position,
        history: &[u64],
        limits: &Limits,
        stop: &AtomicBool,
        on_iteration: &mut dyn FnMut(&IterationInfo),
    ) -> SearchResult {
        let root_moves = generate_legal(root);
        let Some(first_move) = root_moves.iter().next() else {
            let score = if root.in_check() { -MATE } else { 0 };
            return SearchResult {
                best_move: None,
                score,
                depth: 0,
                nodes: 0,
                pv: Vec::new(),
            };
        };
        let mut hashes = Vec::with_capacity(history.len() + MAX_PLY + 1);
        hashes.extend_from_slice(history);
        hashes.push(root.hash());
        let network = self.network.as_deref();
        let mut state = SearchState {
            accumulators: network.map_or_else(Vec::new, |net| {
                vec![Accumulators::new(net, root); MAX_PLY + 2]
            }),
            network,
            tt: &mut self.tt,
            history: &mut self.history,
            correction: &mut self.correction,
            killers: vec![[None, None]; MAX_PLY + 2],
            evals: vec![-INFINITY; MAX_PLY + 2],
            stop,
            limits,
            start: Instant::now(),
            nodes: 0,
            poll_counter: 0,
            seldepth: 0,
            stopped: false,
            root_depth: 0,
            root_index: history.len(),
            hashes,
            pv: (0..=MAX_PLY).map(|_| Vec::with_capacity(MAX_PLY)).collect(),
            after_null: vec![false; MAX_PLY + 2],
        };
        let mut best = SearchResult {
            best_move: Some(first_move),
            score: 0,
            depth: 0,
            nodes: 0,
            pv: vec![first_move],
        };
        let max_depth = limits.depth.unwrap_or(u32::MAX).min(MAX_PLY as u32 - 1);
        let mut score = 0;
        for depth in 1..=max_depth {
            state.root_depth = depth;
            state.seldepth = 0;
            let result = state.aspiration(root, depth as i32, score);
            if state.stopped {
                break;
            }
            score = result;
            let pv = state.pv[0].clone();
            best = SearchResult {
                best_move: pv.first().copied().or(best.best_move),
                score,
                depth,
                nodes: state.nodes,
                pv: pv.clone(),
            };
            on_iteration(&IterationInfo {
                depth,
                seldepth: state.seldepth as u32,
                score,
                nodes: state.nodes,
                elapsed: state.start.elapsed(),
                hashfull: state.tt.hashfull(),
                pv,
            });
            if !should_start_iteration(state.start.elapsed(), limits)
                || state.stop.load(Ordering::Relaxed)
            {
                break;
            }
        }
        best.nodes = state.nodes;
        best
    }
}

/// Estado de uma busca em andamento.
struct SearchState<'a> {
    tt: &'a mut TranspositionTable,
    history: &'a mut History,
    correction: &'a mut CorrectionHistory,
    /// Até dois lances quietos que causaram corte em cada nível.
    killers: Vec<[Option<Move>; 2]>,
    /// Avaliação estática de cada nível do caminho atual (`-INFINITY` quando em xeque).
    evals: Vec<i32>,
    stop: &'a AtomicBool,
    limits: &'a Limits,
    start: Instant,
    nodes: u64,
    poll_counter: u32,
    seldepth: usize,
    stopped: bool,
    root_depth: u32,
    /// Hashes da partida até a raiz seguidos dos do caminho atual; o último é o nó corrente.
    hashes: Vec<u64>,
    root_index: usize,
    /// Tabela triangular de variante principal: `pv[ply]` é a melhor linha a partir de `ply`.
    pv: Vec<Vec<Move>>,
    /// `after_null[ply]`: o nó desse nível foi alcançado por um lance nulo (sem dois seguidos).
    after_null: Vec<bool>,
    network: Option<&'a Network>,
    /// `accumulators[ply]`: camada oculta da rede na posição desse nível (vazio sem rede).
    accumulators: Vec<Accumulators>,
}

impl SearchState<'_> {
    /// Janela estreita em volta da pontuação da iteração anterior, alargada a cada falha.
    fn aspiration(&mut self, root: &Position, depth: i32, previous: i32) -> i32 {
        if depth < 4 {
            return self.negamax(root, depth, -INFINITY, INFINITY, 0, true);
        }
        let mut delta = 25;
        let mut alpha = (previous - delta).max(-INFINITY);
        let mut beta = (previous + delta).min(INFINITY);
        loop {
            let score = self.negamax(root, depth, alpha, beta, 0, true);
            if self.stopped {
                return score;
            }
            if score <= alpha {
                alpha = (score - delta).max(-INFINITY);
            } else if score >= beta {
                beta = (score + delta).min(INFINITY);
            } else {
                return score;
            }
            delta *= 2;
            if delta > 1_000 {
                alpha = -INFINITY;
                beta = INFINITY;
            }
        }
    }

    /// A primeira iteração nunca é interrompida; depois, para por `stop`, limite de nós ou tempo
    /// duro (esses dois últimos checados a cada 1024 chamadas).
    fn should_stop(&mut self) -> bool {
        if self.stopped {
            return true;
        }
        if self.root_depth <= 1 {
            return false;
        }
        if self.limits.nodes.is_some_and(|limit| self.nodes >= limit) {
            self.stopped = true;
            return true;
        }
        self.poll_counter += 1;
        if self.poll_counter >= 1024 {
            self.poll_counter = 0;
            let out_of_time = self
                .limits
                .hard_time
                .is_some_and(|hard| self.start.elapsed() >= hard);
            if out_of_time || self.stop.load(Ordering::Relaxed) {
                self.stopped = true;
            }
        }
        self.stopped
    }

    fn negamax(
        &mut self,
        pos: &Position,
        mut depth: i32,
        mut alpha: i32,
        beta: i32,
        ply: usize,
        pv_node: bool,
    ) -> i32 {
        self.pv[ply].clear();
        if ply > 0 {
            if self.should_stop() {
                return 0;
            }
            if self.is_draw(pos) {
                return 0;
            }
        }
        let in_check = pos.in_check();
        if in_check {
            depth += 1;
        }
        if depth <= 0 {
            return self.quiescence(pos, alpha, beta, ply);
        }
        if ply >= MAX_PLY - 1 {
            return self.raw_eval(pos, ply);
        }
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply);
        self.killers[ply + 1] = [None, None];

        let entry = self.tt.probe(pos.hash());
        let tt_move = entry.and_then(|e| e.mv);
        if let Some(entry) = entry
            && !pv_node
            && entry.depth >= depth
        {
            let score = score_from_tt(entry.score, ply);
            let usable = match entry.bound {
                Bound::Exact => true,
                Bound::Lower => score >= beta,
                Bound::Upper => score <= alpha,
            };
            if usable {
                return score;
            }
        }

        let us = pos.side_to_move();
        // Avaliação estática, já corrigida; em xeque não existe (a posição não é "parada").
        let raw_eval = if in_check {
            -INFINITY
        } else {
            self.raw_eval(pos, ply)
        };
        let static_eval = if in_check {
            -INFINITY
        } else {
            self.corrected_eval(pos, raw_eval)
        };
        self.evals[ply] = static_eval;
        // A posição melhorou em relação à nossa vez anterior? Se sim, podar é mais seguro.
        let improving = !in_check && ply >= 2 && static_eval > self.evals[ply - 2];

        // Sem lance da TT a ordenação é ruim; uma busca um pouco mais rasa sai mais barata.
        if depth >= IIR_MIN_DEPTH && ply > 0 && tt_move.is_none() {
            depth -= 1;
        }

        if !pv_node && !in_check && ply > 0 && beta.abs() < MATE_BOUND {
            // Reverse futility: tão acima de beta que nem uma perda de `margem` por nível muda nada.
            if depth <= RFP_MAX_DEPTH && static_eval - RFP_MARGIN * depth >= beta {
                return static_eval;
            }
            // Null move: se mesmo passando a vez a posição segura beta numa busca rasa, corta.
            // Sem peças além de peões o risco de zugzwang é alto demais.
            if !self.after_null[ply]
                && depth >= NMP_MIN_DEPTH
                && static_eval >= beta
                && pos.has_non_pawn_material(us)
            {
                let reduction = 3 + depth / 3 + ((static_eval - beta) / 200).min(3);
                let null = pos.make_null_move();
                self.hashes.push(null.hash());
                self.push_null(ply);
                self.after_null[ply + 1] = true;
                let score = -self.negamax(
                    &null,
                    depth - 1 - reduction,
                    -beta,
                    -beta + 1,
                    ply + 1,
                    false,
                );
                self.after_null[ply + 1] = false;
                self.hashes.pop();
                if self.stopped {
                    return 0;
                }
                if score >= beta {
                    // Mate achado depois de passar a vez não é prova de mate.
                    return if score >= MATE_BOUND { beta } else { score };
                }
            }
        }

        let mut moves = MoveList::new();
        generate_pseudo_legal(pos, &mut moves);
        let mut scores = [0i32; MAX_MOVES];
        let killers = self.killers[ply];
        score_moves(pos, &moves, tt_move, killers, self.history, &mut scores);

        let original_alpha = alpha;
        let mut best_score = -INFINITY;
        let mut best_move = None;
        let mut legal: usize = 0;
        let mut quiets_tried = MoveList::new();
        for index in 0..moves.len() {
            let mv = pick_next(&mut moves, &mut scores, index);
            let next = pos.make_move(mv);
            if next.is_attacked(next.king_square(us), next.side_to_move()) {
                continue;
            }
            legal += 1;
            let quiet = !is_tactical(pos, mv);
            let new_depth = depth - 1;
            let prunable = !pv_node && !in_check && best_score > -MATE_BOUND && !next.in_check();
            if prunable && depth <= SEE_PRUNE_MAX_DEPTH {
                // Lance que, na troca de peças na casa de destino, perde material demais para a
                // profundidade que resta.
                let margin = if quiet {
                    SEE_QUIET_MARGIN
                } else {
                    SEE_CAPTURE_MARGIN
                };
                if see(pos, mv) < -margin * depth {
                    continue;
                }
            }
            if prunable && quiet {
                // Late move pruning: em nível raso, depois de muitos quietos, o resto quase nunca
                // presta.
                if depth <= LMP_MAX_DEPTH && legal > lmp_threshold(depth, improving) {
                    continue;
                }
                // Futility: nem com uma boa folga a posição chega a alpha com um lance quieto.
                if depth <= FUTILITY_MAX_DEPTH
                    && static_eval + FUTILITY_BASE + FUTILITY_MARGIN * depth <= alpha
                {
                    continue;
                }
            }
            self.hashes.push(next.hash());
            self.push_move(pos, mv, ply);
            let score = if legal == 1 {
                -self.negamax(&next, new_depth, -beta, -alpha, ply + 1, pv_node)
            } else {
                // Lances tardios e quietos: primeiro uma busca reduzida; se surpreender, refaz.
                let late = legal > if pv_node { 3 } else { 2 };
                let reduction =
                    if depth >= LMR_MIN_DEPTH && late && quiet && !in_check && !next.in_check() {
                        let mut r = lmr_reduction(depth, legal);
                        if pv_node {
                            r -= 1;
                        }
                        if killers.contains(&Some(mv)) {
                            r -= 1;
                        }
                        r.clamp(0, new_depth - 1)
                    } else {
                        0
                    };
                let mut score = -self.negamax(
                    &next,
                    new_depth - reduction,
                    -alpha - 1,
                    -alpha,
                    ply + 1,
                    false,
                );
                if reduction > 0 && score > alpha {
                    score = -self.negamax(&next, new_depth, -alpha - 1, -alpha, ply + 1, false);
                }
                if pv_node && score > alpha && score < beta {
                    score = -self.negamax(&next, new_depth, -beta, -alpha, ply + 1, true);
                }
                score
            };
            self.hashes.pop();
            if self.stopped {
                return 0;
            }
            if score > best_score {
                best_score = score;
                if score > alpha {
                    alpha = score;
                    best_move = Some(mv);
                    self.update_pv(ply, mv);
                    if score >= beta {
                        if quiet {
                            self.reward_quiet(us, mv, &quiets_tried, depth, ply);
                        }
                        break;
                    }
                }
            }
            if quiet {
                quiets_tried.push(mv);
            }
        }
        if legal == 0 {
            return if in_check { -MATE + ply as i32 } else { 0 };
        }
        let bound = if best_score >= beta {
            Bound::Lower
        } else if best_score > original_alpha {
            Bound::Exact
        } else {
            Bound::Upper
        };
        let quiet_best = best_move.is_none_or(|mv| !is_tactical(pos, mv));
        if !in_check && quiet_best && correction_applies(bound, best_score, static_eval) {
            self.correction
                .update(us, pos.pawn_hash(), best_score - raw_eval, depth);
        }
        self.tt.store(
            pos.hash(),
            best_move,
            score_to_tt(best_score, ply),
            depth,
            bound,
        );
        best_score
    }

    /// Só capturas e promoções a dama, até a posição "acalmar"; em xeque, todas as evasões.
    fn quiescence(&mut self, pos: &Position, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        self.pv[ply].clear();
        if self.should_stop() {
            return 0;
        }
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply);
        if ply >= MAX_PLY - 1 {
            return self.raw_eval(pos, ply);
        }
        let in_check = pos.in_check();
        let mut best_score = if in_check {
            -INFINITY
        } else {
            let stand_pat = self.corrected_eval(pos, self.raw_eval(pos, ply));
            if stand_pat >= beta {
                return stand_pat;
            }
            alpha = alpha.max(stand_pat);
            stand_pat
        };
        let mut moves = MoveList::new();
        generate_pseudo_legal(pos, &mut moves);
        let mut scores = [0i32; MAX_MOVES];
        score_moves(pos, &moves, None, [None, None], self.history, &mut scores);
        let us = pos.side_to_move();
        let mut legal = 0;
        for index in 0..moves.len() {
            let mv = pick_next(&mut moves, &mut scores, index);
            if !in_check && (!is_tactical(pos, mv) || see(pos, mv) < 0) {
                continue;
            }
            let next = pos.make_move(mv);
            if next.is_attacked(next.king_square(us), next.side_to_move()) {
                continue;
            }
            legal += 1;
            self.push_move(pos, mv, ply);
            let score = -self.quiescence(&next, -beta, -alpha, ply + 1);
            if self.stopped {
                return 0;
            }
            if score > best_score {
                best_score = score;
                if score > alpha {
                    alpha = score;
                    if score >= beta {
                        break;
                    }
                }
            }
        }
        if in_check && legal == 0 {
            return -MATE + ply as i32;
        }
        best_score
    }

    /// Avaliação estática sem correção, do ponto de vista do lado a jogar: a rede, quando há uma,
    /// ou a avaliação à mão.
    fn raw_eval(&self, pos: &Position, ply: usize) -> i32 {
        let score = match self.network {
            Some(net) => {
                let score = net.output(&self.accumulators[ply], pos.side_to_move());
                // Em debug, confere o incremental contra o cálculo do zero, por amostragem para o
                // build de debug seguir jogável (o CI joga partidas com relógio).
                if cfg!(debug_assertions) && self.nodes.is_multiple_of(64) {
                    assert_eq!(score, net.evaluate(pos), "{}", pos.to_fen());
                }
                score
            }
            None => evaluate(pos),
        };
        score.clamp(-MATE_BOUND + 1, MATE_BOUND - 1)
    }

    /// Prepara a camada oculta do nível seguinte para o lance `mv`.
    fn push_move(&mut self, pos: &Position, mv: Move, ply: usize) {
        if let Some(net) = self.network {
            self.accumulators[ply + 1] = self.accumulators[ply].after_move(net, pos, mv);
        }
    }

    /// O lance nulo não mexe em peça: a camada oculta é a mesma.
    fn push_null(&mut self, ply: usize) {
        if self.network.is_some() {
            self.accumulators[ply + 1] = self.accumulators[ply];
        }
    }

    /// Avaliação `raw` somada à correção da estrutura de peões, longe das pontuações de mate.
    fn corrected_eval(&self, pos: &Position, raw: i32) -> i32 {
        let correction = self.correction.get(pos.side_to_move(), pos.pawn_hash());
        (raw + correction).clamp(-MATE_BOUND + 1, MATE_BOUND - 1)
    }

    /// Lance quieto que causou corte: bônus no histórico, punição para os quietos que falharam
    /// antes dele, e vira killer deste nível.
    fn reward_quiet(&mut self, color: Color, mv: Move, tried: &MoveList, depth: i32, ply: usize) {
        let bonus = (16 * depth * depth).min(1_600);
        self.history.update(color, mv, bonus);
        for other in tried {
            self.history.update(color, other, -bonus);
        }
        let slots = &mut self.killers[ply];
        if slots[0] != Some(mv) {
            slots[1] = slots[0];
            slots[0] = Some(mv);
        }
    }

    fn update_pv(&mut self, ply: usize, mv: Move) {
        let (head, tail) = self.pv.split_at_mut(ply + 1);
        let line = &mut head[ply];
        line.clear();
        line.push(mv);
        line.extend_from_slice(&tail[0]);
    }

    /// Empate por regra dos 50 lances, material insuficiente ou repetição.
    fn is_draw(&self, pos: &Position) -> bool {
        if pos.halfmove_clock() >= 100 && (!pos.in_check() || !generate_legal(pos).is_empty()) {
            return true;
        }
        if insufficient_material(pos) {
            return true;
        }
        self.is_repetition(pos)
    }

    /// Repetição depois da raiz já é empate; antes dela, só na terceira ocorrência. Só olha
    /// posições com o mesmo lado a jogar desde o último lance irreversível.
    fn is_repetition(&self, pos: &Position) -> bool {
        let current = self.hashes.len() - 1;
        let key = self.hashes[current];
        let reach = (pos.halfmove_clock() as usize).min(current);
        let mut earlier_occurrences = 0;
        let mut distance = 2;
        while distance <= reach {
            let index = current - distance;
            if self.hashes[index] == key {
                if index > self.root_index {
                    return true;
                }
                earlier_occurrences += 1;
                if earlier_occurrences >= 2 {
                    return true;
                }
            }
            distance += 2;
        }
        false
    }
}

/// Sem peões, torres ou damas e com no máximo uma peça menor: ninguém consegue dar mate.
pub(crate) fn insufficient_material(pos: &Position) -> bool {
    use crate::types::Color::{Black, White};
    let heavy_or_pawn = [PieceType::Pawn, PieceType::Rook, PieceType::Queen]
        .into_iter()
        .any(|kind| !(pos.pieces(White, kind) | pos.pieces(Black, kind)).is_empty());
    if heavy_or_pawn {
        return false;
    }
    let minors = [PieceType::Knight, PieceType::Bishop]
        .into_iter()
        .map(|kind| (pos.pieces(White, kind) | pos.pieces(Black, kind)).count())
        .sum::<u32>();
    minors <= 1
}

const ORDER_VALUE: [i32; 6] = [1, 3, 3, 5, 9, 0];

fn captured_kind(pos: &Position, mv: Move) -> Option<PieceType> {
    match mv.kind() {
        MoveKind::EnPassant => Some(PieceType::Pawn),
        MoveKind::Castle => None,
        _ => pos.piece_at(mv.to()).map(|piece| piece.kind),
    }
}

/// Capturas e promoções a dama: os lances da busca quiescente.
pub(crate) fn is_tactical(pos: &Position, mv: Move) -> bool {
    captured_kind(pos, mv).is_some() || mv.kind() == MoveKind::Promotion(PieceType::Queen)
}

/// Ordem: lance da TT, capturas por MVV-LVA (vítima mais valiosa, atacante mais barato),
/// promoções a dama, demais lances.
fn score_moves(
    pos: &Position,
    moves: &MoveList,
    tt_move: Option<Move>,
    killers: [Option<Move>; 2],
    history: &History,
    scores: &mut [i32],
) {
    let us = pos.side_to_move();
    for (score, mv) in scores.iter_mut().zip(moves.iter()) {
        *score = if Some(mv) == tt_move {
            1_000_000
        } else if let Some(victim) = captured_kind(pos, mv) {
            let attacker = pos
                .piece_at(mv.from())
                .map_or(0, |p| ORDER_VALUE[p.kind.index()]);
            let mvv_lva = 10 * ORDER_VALUE[victim.index()] - attacker;
            if see(pos, mv) >= 0 {
                100_000 + mvv_lva
            } else {
                -100_000 + mvv_lva
            }
        } else if mv.kind() == MoveKind::Promotion(PieceType::Queen) {
            90_000
        } else if Some(mv) == killers[0] {
            80_000
        } else if Some(mv) == killers[1] {
            79_000
        } else {
            history.get(us, mv)
        };
    }
}

/// Seleção: traz para `index` o lance de maior pontuação ainda não visitado.
fn pick_next(moves: &mut MoveList, scores: &mut [i32], index: usize) -> Move {
    let len = moves.len();
    let best = (index..len)
        .max_by_key(|&i| scores[i])
        .expect("índice dentro da lista");
    moves.as_mut_slice().swap(index, best);
    scores.swap(index, best);
    moves.as_slice()[index]
}

/// Texto do campo `score` do UCI: `cp N` ou `mate N` (N em lances, negativo quando levamos mate).
pub fn uci_score(score: i32) -> String {
    if score >= MATE_BOUND {
        format!("mate {}", (MATE - score + 1) / 2)
    } else if score <= -MATE_BOUND {
        format!("mate -{}", (MATE + score) / 2)
    } else {
        format!("cp {score}")
    }
}

/// Mates são guardados na TT relativos à posição, não à raiz.
fn score_to_tt(score: i32, ply: usize) -> i32 {
    if score >= MATE_BOUND {
        score + ply as i32
    } else if score <= -MATE_BOUND {
        score - ply as i32
    } else {
        score
    }
}

fn score_from_tt(score: i32, ply: usize) -> i32 {
    if score >= MATE_BOUND {
        score - ply as i32
    } else if score <= -MATE_BOUND {
        score + ply as i32
    } else {
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::generate_legal;

    fn run(fen: &str, limits: Limits) -> SearchResult {
        let pos = Position::from_fen(fen).unwrap();
        let mut searcher = Searcher::new(16);
        searcher.search(&pos, &[], &limits, &AtomicBool::new(false), &mut |_| {})
    }

    fn depth(d: u32) -> Limits {
        Limits {
            depth: Some(d),
            ..Limits::default()
        }
    }

    fn uci(mv: Option<Move>) -> String {
        mv.expect("sem lance").to_uci(false)
    }

    #[test]
    fn finds_back_rank_mate_in_one() {
        let result = run("6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1", depth(3));
        assert_eq!(uci(result.best_move), "d1d8");
        assert_eq!(result.score, MATE - 1);
        assert_eq!(uci_score(result.score), "mate 1");
    }

    #[test]
    fn finds_mate_in_two() {
        // Problema clássico de Morphy: 1.Ra6! e 2.b7# ou 2.Rxa7#.
        let result = run("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1", depth(5));
        assert_eq!(uci(result.best_move), "a1a6");
        assert_eq!(result.score, MATE - 3);
        assert_eq!(uci_score(result.score), "mate 2");
    }

    #[test]
    fn sees_being_mated() {
        // Único lance das pretas é Kb8, e então Rh8#.
        let result = run("k7/8/1K6/8/8/8/8/7R b - - 0 1", depth(4));
        assert_eq!(result.score, -(MATE - 2));
        assert_eq!(uci_score(result.score), "mate -1");
    }

    #[test]
    fn captures_a_hanging_queen() {
        let result = run("4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1", depth(3));
        assert_eq!(uci(result.best_move), "d1d5");
    }

    #[test]
    fn insufficient_material_is_a_draw() {
        assert_eq!(run("8/8/4k3/8/8/3NK3/8/8 w - - 0 1", depth(6)).score, 0);
        assert_eq!(run("8/8/4k3/8/8/4K3/8/8 b - - 0 1", depth(6)).score, 0);
    }

    #[test]
    fn fifty_move_rule_is_a_draw() {
        // Uma dama a mais, mas qualquer lance completa 100 meios-lances sem captura nem peão.
        let result = run("4k3/8/8/8/8/8/8/Q3K3 w - - 99 80", depth(3));
        assert_eq!(result.score, 0);
    }

    #[test]
    fn threefold_repetition_is_a_draw() {
        // As pretas, com um cavalo contra uma dama, repetem a posição inicial pela terceira vez
        // com Ng8 e empatam, em vez de ficarem perdidas.
        let mut pos = Position::from_fen("4k1n1/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        let mut history = Vec::new();
        for uci in ["d1d2", "g8f6", "d2d1", "f6g8", "d1d2", "g8f6", "d2d1"] {
            history.push(pos.hash());
            let mv = generate_legal(&pos)
                .iter()
                .find(|m| m.to_uci(false) == uci)
                .unwrap();
            pos = pos.make_move(mv);
        }
        let mut searcher = Searcher::new(16);
        let result = searcher.search(
            &pos,
            &history,
            &depth(4),
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert_eq!(uci(result.best_move), "f6g8");
        assert_eq!(result.score, 0);
    }

    #[test]
    fn respects_depth_and_node_limits() {
        let result = run(crate::position::STARTPOS_FEN, depth(4));
        assert_eq!(result.depth, 4);
        let nodes = Limits {
            nodes: Some(20_000),
            ..Limits::default()
        };
        let result = run(crate::position::STARTPOS_FEN, nodes);
        assert!(result.nodes <= 22_000, "nós {}", result.nodes);
        assert!(result.best_move.is_some());
    }

    #[test]
    fn stops_on_hard_time_limit() {
        let limits = Limits {
            hard_time: Some(Duration::from_millis(100)),
            soft_time: Some(Duration::from_millis(100)),
            ..Limits::default()
        };
        let start = Instant::now();
        let result = run(crate::position::STARTPOS_FEN, limits);
        assert!(
            start.elapsed() < Duration::from_millis(1_000),
            "{:?}",
            start.elapsed()
        );
        assert!(result.best_move.is_some());
    }

    #[test]
    fn returns_a_legal_move_even_when_already_stopped() {
        let pos = Position::startpos();
        let mut searcher = Searcher::new(16);
        let result = searcher.search(
            &pos,
            &[],
            &Limits::default(),
            &AtomicBool::new(true),
            &mut |_| {},
        );
        let best = result.best_move.expect("sem lance");
        assert!(generate_legal(&pos).contains(best));
    }

    #[test]
    fn reports_every_completed_iteration() {
        let pos = Position::startpos();
        let mut searcher = Searcher::new(16);
        let mut depths = Vec::new();
        let result = searcher.search(&pos, &[], &depth(5), &AtomicBool::new(false), &mut |info| {
            depths.push(info.depth);
            assert!(!info.pv.is_empty());
        });
        assert_eq!(depths, vec![1, 2, 3, 4, 5]);
        assert_eq!(result.pv.first().copied(), result.best_move);
    }

    #[test]
    fn mate_scores_survive_the_tt_round_trip() {
        for ply in [0, 3, 17] {
            for score in [MATE - 5, -(MATE - 8), 150, -42, 0] {
                assert_eq!(score_from_tt(score_to_tt(score, ply), ply), score);
            }
        }
        // Mate em 5 meios-lances visto a 3 da raiz fica guardado como mate em 2 a partir do nó.
        assert_eq!(score_to_tt(MATE - 5, 3), MATE - 2);
    }

    fn quiet(from: &str, to: &str) -> Move {
        Move::new(from.parse().unwrap(), to.parse().unwrap(), MoveKind::Normal)
    }

    #[test]
    fn history_gravity_keeps_values_bounded() {
        let mut history = History::new();
        let mv = quiet("g1", "f3");
        for _ in 0..1_000 {
            history.update(Color::White, mv, 1_600);
        }
        let high = history.get(Color::White, mv);
        assert!((10_001..=HISTORY_MAX).contains(&high), "{high}");
        for _ in 0..1_000 {
            history.update(Color::White, mv, -1_600);
        }
        let low = history.get(Color::White, mv);
        assert!((-HISTORY_MAX..-10_000).contains(&low), "{low}");
        assert_eq!(history.get(Color::Black, mv), 0);
    }

    #[test]
    fn correction_follows_the_search_error_and_stays_bounded() {
        let mut correction = CorrectionHistory::new();
        let pawns = 0x1234_5678_9ABC_DEF0;
        assert_eq!(correction.get(Color::White, pawns), 0);
        correction.update(Color::White, pawns, 40, 8);
        let once = correction.get(Color::White, pawns);
        assert!((1..40).contains(&once), "{once}");
        for _ in 0..300 {
            correction.update(Color::White, pawns, 40, 8);
        }
        let settled = correction.get(Color::White, pawns);
        assert!((38..=40).contains(&settled), "{settled}");
        // Erro enorme: a correção encosta no teto, sem nunca passar dele.
        for _ in 0..300 {
            correction.update(Color::White, pawns, 5_000, 20);
            assert!(correction.get(Color::White, pawns) <= CORRECTION_MAX);
        }
        assert!(correction.get(Color::White, pawns) >= CORRECTION_MAX - 1);
        for _ in 0..300 {
            correction.update(Color::White, pawns, -5_000, 20);
            assert!(correction.get(Color::White, pawns) >= -CORRECTION_MAX);
        }
        assert!(correction.get(Color::White, pawns) <= -CORRECTION_MAX + 1);
        // Outra cor e outra estrutura de peões não são afetadas.
        assert_eq!(correction.get(Color::Black, pawns), 0);
        assert_eq!(correction.get(Color::White, pawns ^ 1), 0);
    }

    #[test]
    fn deeper_searches_move_the_correction_more() {
        let mut shallow = CorrectionHistory::new();
        let mut deep = CorrectionHistory::new();
        shallow.update(Color::White, 7, 100, 1);
        deep.update(Color::White, 7, 100, 10);
        assert!(deep.get(Color::White, 7) > shallow.get(Color::White, 7));
    }

    #[test]
    fn correction_only_learns_when_the_bound_proves_the_direction() {
        assert!(correction_applies(Bound::Exact, 10, 50));
        // Falha alta: o valor real é pelo menos `best_score`; só ensina se passou da avaliação.
        assert!(correction_applies(Bound::Lower, 80, 50));
        assert!(!correction_applies(Bound::Lower, 30, 50));
        // Falha baixa: o valor real é no máximo `best_score`.
        assert!(correction_applies(Bound::Upper, 30, 50));
        assert!(!correction_applies(Bound::Upper, 80, 50));
        // Mate não é erro de avaliação.
        assert!(!correction_applies(Bound::Exact, MATE - 3, 0));
    }

    #[test]
    fn lmr_reduction_grows_with_depth_and_move_number() {
        assert_eq!(lmr_reduction(1, 1), 0);
        assert!(lmr_reduction(3, 4) >= 1);
        for depth in 1..40 {
            for number in 1..60 {
                assert!(lmr_reduction(depth + 1, number) >= lmr_reduction(depth, number));
                assert!(lmr_reduction(depth, number + 1) >= lmr_reduction(depth, number));
            }
        }
        assert!(lmr_reduction(20, 40) >= 4);
    }

    #[test]
    fn move_ordering_puts_killers_between_captures_and_quiets() {
        // Brancas: Nxe5 ganha um peão solto; Nf3-g5 é killer; Bc4-b5 tem histórico; a2-a3 não
        // tem nada; Bxf7+ perde o bispo para o rei (captura ruim, vai para depois dos quietos).
        let pos =
            Position::from_fen("rnbqkbnr/pppp1ppp/8/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 1")
                .unwrap();
        let mut moves = MoveList::new();
        generate_pseudo_legal(&pos, &mut moves);
        let mut history = History::new();
        history.update(Color::White, quiet("c4", "b5"), 900);
        let killer = quiet("f3", "g5");
        let mut scores = [0i32; MAX_MOVES];
        score_moves(
            &pos,
            &moves,
            None,
            [Some(killer), None],
            &history,
            &mut scores,
        );
        let score_of = |uci: &str| {
            let i = moves
                .iter()
                .position(|m| m.to_uci(false) == uci)
                .unwrap_or_else(|| panic!("{uci} não gerado"));
            scores[i]
        };
        assert!(score_of("f3e5") > score_of("f3g5"));
        assert!(score_of("f3g5") > score_of("c4b5"));
        assert!(score_of("c4b5") > score_of("a2a3"));
        assert!(score_of("a2a3") > score_of("c4f7"));
    }

    #[test]
    fn with_a_network_the_search_evaluates_with_it() {
        // Em debug, cada avaliação incremental é conferida contra a avaliação do zero.
        let fens = [
            crate::position::STARTPOS_FEN,
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9",
            "4k3/1P6/8/8/8/8/6p1/4K3 w - - 0 1",
        ];
        // Limite de nós: com uma rede aleatória a busca quiescente não "acalma" e a profundidade
        // não serve de régua de custo.
        let nodes = Limits {
            nodes: Some(5_000),
            ..Limits::default()
        };
        for fen in fens {
            let pos = Position::from_fen(fen).unwrap();
            let search = |network: Option<u64>| {
                let mut searcher = Searcher::new(16);
                searcher
                    .set_network(network.map(|seed| Arc::new(crate::nnue::random_network(seed))));
                searcher.search(&pos, &[], &nodes, &AtomicBool::new(false), &mut |_| {})
            };
            let with_net = search(Some(5));
            assert!(
                generate_legal(&pos).contains(with_net.best_move.unwrap()),
                "{fen}"
            );
            // Outra rede (ou nenhuma) avalia diferente: a rede está mesmo sendo usada.
            let (other, none) = (search(Some(6)), search(None));
            assert!(
                with_net.score != other.score || with_net.score != none.score,
                "{fen}"
            );
        }
    }

    #[test]
    fn uci_score_text() {
        assert_eq!(uci_score(35), "cp 35");
        assert_eq!(uci_score(-120), "cp -120");
        assert_eq!(uci_score(MATE - 1), "mate 1");
        assert_eq!(uci_score(MATE - 4), "mate 2");
        assert_eq!(uci_score(-(MATE - 2)), "mate -1");
    }
}
