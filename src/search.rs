//! Busca da v1: negamax alpha-beta fail-soft com aprofundamento iterativo, janelas de aspiração,
//! PVS, extensão de xeque, busca quiescente, tabela de transposição e ordenação TT → MVV-LVA.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::eval::evaluate;
use crate::movegen::{generate_legal, generate_pseudo_legal};
use crate::moves::{MAX_MOVES, Move, MoveKind, MoveList};
use crate::position::Position;
use crate::timeman::Limits;
use crate::tt::{Bound, TranspositionTable};
use crate::types::PieceType;

pub const MAX_PLY: usize = 128;
pub const INFINITY: i32 = 32_000;
pub const MATE: i32 = 31_000;
/// Pontuações além disso são mate em até `MAX_PLY` meios-lances.
pub const MATE_BOUND: i32 = MATE - MAX_PLY as i32;

const RFP_MAX_DEPTH: i32 = 8;
const RFP_MARGIN: i32 = 80;
const NMP_MIN_DEPTH: i32 = 3;

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

pub struct Searcher {
    tt: TranspositionTable,
}

impl Searcher {
    pub fn new(hash_megabytes: usize) -> Searcher {
        Searcher {
            tt: TranspositionTable::new(hash_megabytes),
        }
    }

    pub fn resize(&mut self, hash_megabytes: usize) {
        self.tt = TranspositionTable::new(hash_megabytes);
    }

    pub fn clear(&mut self) {
        self.tt.clear();
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
        let mut state = SearchState {
            tt: &mut self.tt,
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
            let soft_limit_reached = limits
                .soft_time
                .is_some_and(|soft| state.start.elapsed() >= soft);
            if soft_limit_reached || state.stop.load(Ordering::Relaxed) {
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
            return evaluate(pos);
        }
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply);

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
        if !pv_node && !in_check && ply > 0 && beta.abs() < MATE_BOUND {
            let static_eval = evaluate(pos);
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
        score_moves(pos, &moves, tt_move, &mut scores);

        let original_alpha = alpha;
        let mut best_score = -INFINITY;
        let mut best_move = None;
        let mut legal = 0;
        for index in 0..moves.len() {
            let mv = pick_next(&mut moves, &mut scores, index);
            let next = pos.make_move(mv);
            if next.is_attacked(next.king_square(us), next.side_to_move()) {
                continue;
            }
            legal += 1;
            self.hashes.push(next.hash());
            let score = if legal == 1 {
                -self.negamax(&next, depth - 1, -beta, -alpha, ply + 1, pv_node)
            } else {
                let mut score = -self.negamax(&next, depth - 1, -alpha - 1, -alpha, ply + 1, false);
                if pv_node && score > alpha && score < beta {
                    score = -self.negamax(&next, depth - 1, -beta, -alpha, ply + 1, true);
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
                        break;
                    }
                }
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
            return evaluate(pos);
        }
        let in_check = pos.in_check();
        let mut best_score = if in_check {
            -INFINITY
        } else {
            let stand_pat = evaluate(pos);
            if stand_pat >= beta {
                return stand_pat;
            }
            alpha = alpha.max(stand_pat);
            stand_pat
        };
        let mut moves = MoveList::new();
        generate_pseudo_legal(pos, &mut moves);
        let mut scores = [0i32; MAX_MOVES];
        score_moves(pos, &moves, None, &mut scores);
        let us = pos.side_to_move();
        let mut legal = 0;
        for index in 0..moves.len() {
            let mv = pick_next(&mut moves, &mut scores, index);
            if !in_check && !is_tactical(pos, mv) {
                continue;
            }
            let next = pos.make_move(mv);
            if next.is_attacked(next.king_square(us), next.side_to_move()) {
                continue;
            }
            legal += 1;
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
fn insufficient_material(pos: &Position) -> bool {
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
fn is_tactical(pos: &Position, mv: Move) -> bool {
    captured_kind(pos, mv).is_some() || mv.kind() == MoveKind::Promotion(PieceType::Queen)
}

/// Ordem: lance da TT, capturas por MVV-LVA (vítima mais valiosa, atacante mais barato),
/// promoções a dama, demais lances.
fn score_moves(pos: &Position, moves: &MoveList, tt_move: Option<Move>, scores: &mut [i32]) {
    for (score, mv) in scores.iter_mut().zip(moves.iter()) {
        *score = if Some(mv) == tt_move {
            1_000_000
        } else if let Some(victim) = captured_kind(pos, mv) {
            let attacker = pos
                .piece_at(mv.from())
                .map_or(0, |p| ORDER_VALUE[p.kind.index()]);
            100_000 + 10 * ORDER_VALUE[victim.index()] - attacker
        } else if mv.kind() == MoveKind::Promotion(PieceType::Queen) {
            90_000
        } else {
            0
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

    #[test]
    fn uci_score_text() {
        assert_eq!(uci_score(35), "cp 35");
        assert_eq!(uci_score(-120), "cp -120");
        assert_eq!(uci_score(MATE - 1), "mate 1");
        assert_eq!(uci_score(MATE - 4), "mate 2");
        assert_eq!(uci_score(-(MATE - 2)), "mate -1");
    }
}
