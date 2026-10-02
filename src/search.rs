//! Busca da v1: negamax alpha-beta fail-soft com aprofundamento iterativo, janelas de aspiração,
//! PVS, extensão de xeque, busca quiescente, tabela de transposição e ordenação TT → MVV-LVA.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use crate::bitboard::Bitboard;
use crate::eval::evaluate;
use crate::movegen::{generate_legal, generate_pseudo_legal};
use crate::moves::{MAX_MOVES, Move, MoveKind, MoveList};
use crate::nnue::{Accumulators, Network, RefreshCache};
use crate::position::Position;
use crate::see::{SEE_VALUE, see};
use crate::timeman::{Limits, iteration_time_scale, should_start_iteration_scaled};
use crate::tt::{Bound, TranspositionTable};
use crate::tune;
use crate::types::{Color, Piece, PieceType, Square};

pub const MAX_PLY: usize = 128;
pub const INFINITY: i32 = 32_000;
pub const MATE: i32 = 31_000;
/// Pontuações além disso são mate em até `MAX_PLY` meios-lances.
pub const MATE_BOUND: i32 = MATE - MAX_PLY as i32;

// Margens, profundidades e fórmulas da poda, das reduções e dos históricos: parâmetros de
// `tune.rs` (constantes no build normal, opções UCI com a feature `tune`, para o SPSA). O que
// cada um faz está comentado na tabela de lá.

/// Quantos lances um nó de profundidade `depth` busca antes de o LMP podar os quietos restantes.
fn lmp_threshold(depth: i32, improving: bool) -> usize {
    let base = (tune::lmp_base() + depth * depth) as usize;
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

/// Histórico de lances quietos ("butterfly"): quanto cada lance (cor, origem, destino) causou
/// cortes, separado por a origem e o destino estarem ou não atacados pelo adversário (`threats`):
/// fugir de um ataque e entrar numa casa atacada são lances de natureza bem diferente.
struct History {
    table: [[[[i32; 64]; 64]; 4]; 2],
}

impl History {
    fn new() -> Box<History> {
        Box::new(History {
            table: [[[[0; 64]; 64]; 4]; 2],
        })
    }

    /// Balde de ameaça do lance: bit 1 se a origem está atacada, bit 0 se o destino está.
    fn bucket(mv: Move, threats: Bitboard) -> usize {
        2 * usize::from(threats.contains(mv.from())) + usize::from(threats.contains(mv.to()))
    }

    fn get(&self, color: Color, mv: Move, threats: Bitboard) -> i32 {
        self.table[color.index()][Self::bucket(mv, threats)][mv.from().index()][mv.to().index()]
    }

    /// Soma `bonus` (negativo para punir) com gravidade: nunca passa de `tune::history_max()`.
    fn update(&mut self, color: Color, mv: Move, threats: Bitboard, bonus: i32) {
        apply_bonus(
            &mut self.table[color.index()][Self::bucket(mv, threats)][mv.from().index()]
                [mv.to().index()],
            bonus,
        );
    }

    fn clear(&mut self) {
        self.table = [[[[0; 64]; 64]; 4]; 2];
    }
}

/// Soma `bonus` (negativo para punir) com gravidade: quanto mais perto do teto, menos o valor
/// anda naquela direção, e nunca passa de `tune::history_max()`.
fn apply_bonus(entry: &mut i32, bonus: i32) {
    let max = tune::history_max();
    let bonus = bonus.clamp(-max, max);
    *entry += bonus - *entry * bonus.abs() / max;
}

/// Um lance visto só pela peça que se moveu e pela casa de destino.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PieceTo {
    piece: Piece,
    to: Square,
}

impl PieceTo {
    fn index(self) -> usize {
        (self.piece.color.index() * 6 + self.piece.kind.index()) * 64 + self.to.index()
    }
}

/// Uma tabela de histórico por (peça, destino) da resposta.
type PieceToTable = [[i32; 64]; 12];

/// Histórico de continuação: para cada lance anterior (peça, destino), quais respostas
/// (peça, destino) causaram corte. Um lance quieto costuma ser bom ou ruim por causa do que
/// acabou de acontecer no tabuleiro, e o histórico simples não enxerga isso.
struct ContinuationHistory {
    tables: Vec<PieceToTable>,
}

impl ContinuationHistory {
    fn new() -> Box<ContinuationHistory> {
        Box::new(ContinuationHistory {
            tables: vec![[[0; 64]; 12]; 12 * 64],
        })
    }

    fn get(&self, previous: PieceTo, piece: Piece, to: Square) -> i32 {
        let reply = PieceTo { piece, to }.index();
        self.tables[previous.index()][reply / 64][reply % 64]
    }

    fn update(&mut self, previous: PieceTo, piece: Piece, to: Square, bonus: i32) {
        let reply = PieceTo { piece, to }.index();
        apply_bonus(
            &mut self.tables[previous.index()][reply / 64][reply % 64],
            bonus,
        );
    }

    fn clear(&mut self) {
        self.tables.fill([[0; 64]; 12]);
    }
}

/// Uma captura vista pela peça que captura, a casa de destino, o tipo da vítima e se a casa já
/// era atacada pelo adversário antes do lance (captura que pode ser respondida na hora).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CaptureKey {
    piece: Piece,
    to: Square,
    victim: PieceType,
    threatened: bool,
}

impl CaptureKey {
    /// Chave do lance `mv` em `pos`; `None` se ele não captura nada.
    fn of(pos: &Position, mv: Move) -> Option<CaptureKey> {
        let victim = captured_kind(pos, mv)?;
        let piece = pos.piece_at(mv.from())?;
        Some(CaptureKey {
            piece,
            to: mv.to(),
            victim,
            threatened: pos.is_attacked(mv.to(), piece.color.flip()),
        })
    }

    fn index(self) -> usize {
        let piece_to = PieceTo {
            piece: self.piece,
            to: self.to,
        }
        .index();
        (piece_to * 6 + self.victim.index()) * 2 + usize::from(self.threatened)
    }
}

/// Histórico de capturas: quanto cada captura (ver `CaptureKey`) causou cortes. Corrige a ordem
/// por vítima e a fronteira entre capturas boas e ruins com o que a busca já viu.
struct CaptureHistory {
    table: Vec<i32>,
}

impl CaptureHistory {
    fn new() -> Box<CaptureHistory> {
        Box::new(CaptureHistory {
            table: vec![0; 12 * 64 * 6 * 2],
        })
    }

    fn get(&self, key: CaptureKey) -> i32 {
        self.table[key.index()]
    }

    /// Soma `bonus` (negativo para punir) com gravidade, como o `History`.
    fn update(&mut self, key: CaptureKey, bonus: i32) {
        apply_bonus(&mut self.table[key.index()], bonus);
    }

    fn clear(&mut self) {
        self.table.fill(0);
    }
}

/// Fim de um nó com corte: a captura `best` (se o lance do corte foi captura) ganha `bonus` no
/// histórico de capturas; as capturas tentadas antes dele, que não cortaram, perdem o mesmo.
fn reward_captures(
    captures: &mut CaptureHistory,
    pos: &Position,
    best: Option<Move>,
    tried: &MoveList,
    bonus: i32,
) {
    if let Some(key) = best.and_then(|mv| CaptureKey::of(pos, mv)) {
        captures.update(key, bonus);
    }
    for key in tried.iter().filter_map(|mv| CaptureKey::of(pos, mv)) {
        captures.update(key, -bonus);
    }
}

/// O que ordena os lances de um nó além da TT e dos killers. Quietos: o histórico simples (no
/// balde de ameaça do lance) e as continuações dos lances anteriores: o do adversário e o nosso
/// (`previous[0]` e `previous[1]`, numa tabela) e o nosso de quatro meios-lances atrás
/// (`previous[2]`, noutra). Capturas: o histórico de capturas.
struct MoveOrdering<'a> {
    history: &'a History,
    continuation: &'a ContinuationHistory,
    continuation4: &'a ContinuationHistory,
    captures: &'a CaptureHistory,
    /// Casas atacadas pelo adversário na posição do nó.
    threats: Bitboard,
    previous: [Option<PieceTo>; 3],
}

impl MoveOrdering<'_> {
    fn quiet_score(&self, pos: &Position, mv: Move) -> i32 {
        let mut score = self.history.get(pos.side_to_move(), mv, self.threats);
        if let Some(piece) = pos.piece_at(mv.from()) {
            for previous in self.previous[..2].iter().flatten() {
                score += self.continuation.get(*previous, piece, mv.to());
            }
            if let Some(previous) = self.previous[2] {
                // Lance mais distante, sinal mais fraco: entra pela metade.
                score += self.continuation4.get(previous, piece, mv.to()) / 2;
            }
        }
        score
    }
}

/// Entradas por cor das tabelas de correção indexadas por hash (potência de 2).
const CORRECTION_SIZE: usize = 16_384;
/// As entradas guardam centipeões multiplicados por isto, para a média móvel não perder precisão.
const CORRECTION_GRAIN: i32 = 256;
/// Entradas por cor da correção de continuação: cobre todo par (lance do adversário, nosso lance
/// antes dele), ver `continuation_key`.
const CONTINUATION_CORRECTION_SIZE: usize = 1 << 18;
/// Peso de cada tabela na correção final, em `1/CORRECTION_WEIGHT_SCALE`. A soma passa um pouco
/// de 1: as tabelas enxergam partes diferentes da posição e raramente concordam por inteiro.
const CORRECTION_WEIGHT_SCALE: i32 = 128;
const CORRECTION_WEIGHT_PAWN: i32 = 56;
const CORRECTION_WEIGHT_NON_PAWN: i32 = 36;
const CORRECTION_WEIGHT_CONTINUATION: i32 = 40;

/// Média móvel, por lado a jogar e por chave, da diferença entre o que a busca achou e o que a
/// avaliação dizia. Conserta, aos poucos, o que a avaliação erra de forma sistemática naquele
/// tipo de posição.
struct CorrectionHistory {
    /// `entries` por cor, uma cor depois da outra.
    table: Vec<i32>,
    entries: usize,
}

impl CorrectionHistory {
    /// `entries` por cor, potência de 2.
    fn new(entries: usize) -> CorrectionHistory {
        debug_assert!(entries.is_power_of_two());
        CorrectionHistory {
            table: vec![0; 2 * entries],
            entries,
        }
    }

    fn index(&self, color: Color, key: u64) -> usize {
        color.index() * self.entries + (key as usize & (self.entries - 1))
    }

    /// Correção, em centipeões vezes `CORRECTION_GRAIN`, para o lado `color` a jogar.
    fn raw(&self, color: Color, key: u64) -> i32 {
        self.table[self.index(color, key)]
    }

    /// Correção, em centipeões, para o lado `color` a jogar (a busca usa a soma de `Corrections`).
    #[cfg(test)]
    fn get(&self, color: Color, key: u64) -> i32 {
        self.raw(color, key) / CORRECTION_GRAIN
    }

    /// Puxa a entrada na direção de `error` (busca menos avaliação crua); buscas mais fundas
    /// pesam mais.
    fn update(&mut self, color: Color, key: u64, error: i32, depth: i32) {
        let weight = (depth + 1).clamp(1, 16);
        let limit = tune::correction_max() * CORRECTION_GRAIN;
        let target = (error * CORRECTION_GRAIN).clamp(-limit, limit);
        let index = self.index(color, key);
        let entry = &mut self.table[index];
        *entry += (target - *entry) * weight / 256;
    }

    fn clear(&mut self) {
        self.table.fill(0);
    }
}

/// As chaves de um nó em cada tabela de correção.
#[derive(Clone, Copy, Debug)]
struct CorrectionKeys {
    pawn: u64,
    /// Peças (menos peões) de cada cor, indexado pela cor.
    non_pawn: [u64; 2],
    /// `None` quando o nó não veio de um lance de peça (raiz, lance nulo).
    continuation: Option<usize>,
}

/// Chave da correção de continuação: o lance do adversário que levou ao nó e o nosso antes dele.
/// A cor de cada um já é implícita (o primeiro é sempre do adversário), então só peça e destino
/// entram; sem lance do adversário não há chave.
fn continuation_key(previous: [Option<PieceTo>; 2]) -> Option<usize> {
    const PIECE_TO: usize = 6 * 64;
    let theirs = previous[0]?.index() % PIECE_TO;
    let ours = previous[1].map_or(PIECE_TO, |mv| mv.index() % PIECE_TO);
    Some(theirs * (PIECE_TO + 1) + ours)
}

/// Correções da avaliação estática pela estrutura de peões, pelas peças de cada cor e pelos dois
/// últimos lances, somadas com pesos.
struct Corrections {
    pawn: CorrectionHistory,
    non_pawn: [CorrectionHistory; 2],
    continuation: CorrectionHistory,
}

impl Corrections {
    fn new() -> Box<Corrections> {
        Box::new(Corrections {
            pawn: CorrectionHistory::new(CORRECTION_SIZE),
            non_pawn: [
                CorrectionHistory::new(CORRECTION_SIZE),
                CorrectionHistory::new(CORRECTION_SIZE),
            ],
            continuation: CorrectionHistory::new(CONTINUATION_CORRECTION_SIZE),
        })
    }

    /// Correção total, em centipeões, para o lado `color` a jogar.
    fn get(&self, color: Color, keys: &CorrectionKeys) -> i32 {
        let mut sum = CORRECTION_WEIGHT_PAWN * self.pawn.raw(color, keys.pawn);
        for (table, key) in self.non_pawn.iter().zip(keys.non_pawn) {
            sum += CORRECTION_WEIGHT_NON_PAWN * table.raw(color, key);
        }
        if let Some(key) = keys.continuation {
            sum += CORRECTION_WEIGHT_CONTINUATION * self.continuation.raw(color, key as u64);
        }
        sum / (CORRECTION_WEIGHT_SCALE * CORRECTION_GRAIN)
    }

    /// Cada tabela aprende o erro inteiro; os pesos só entram na hora de aplicar.
    fn update(&mut self, color: Color, keys: &CorrectionKeys, error: i32, depth: i32) {
        self.pawn.update(color, keys.pawn, error, depth);
        for (table, key) in self.non_pawn.iter_mut().zip(keys.non_pawn) {
            table.update(color, key, error, depth);
        }
        if let Some(key) = keys.continuation {
            self.continuation.update(color, key as u64, error, depth);
        }
    }

    fn clear(&mut self) {
        self.pawn.clear();
        for table in &mut self.non_pawn {
            table.clear();
        }
        self.continuation.clear();
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

/// Bônus (e punição) de histórico por um corte a profundidade `depth`.
fn history_bonus(depth: i32) -> i32 {
    (tune::history_bonus_mul() * depth * depth).min(tune::history_bonus_max())
}

/// Redução base do LMR, de quietos ou de táticos, para a profundidade e o número do lance (1 =
/// primeiro lance legal). Com a feature `tune` as fórmulas mudam entre partidas: calcula na hora
/// em vez de ler a tabela.
fn base_reduction(depth: i32, move_number: usize, tactical: bool) -> i32 {
    let depth = (depth.max(0) as usize).min(63);
    let number = move_number.min(63);
    if cfg!(feature = "tune") {
        lmr_formula(depth, number, tactical)
    } else if tactical {
        LMR_TACTICAL_TABLE[depth][number]
    } else {
        LMR_TABLE[depth][number]
    }
}

/// `base/100 + ln(profundidade)·ln(número do lance)/(divisor/100)`, truncado; zero na linha e na
/// coluna 0. Quietos: 0,75 e 2,25 por padrão; táticos reduzem bem menos (−0,25 e 3,5).
fn lmr_formula(depth: usize, number: usize, tactical: bool) -> i32 {
    if depth == 0 || number == 0 {
        return 0;
    }
    let (base, divisor) = if tactical {
        (tune::lmr_tactical_base(), tune::lmr_tactical_divisor())
    } else {
        (tune::lmr_base(), tune::lmr_divisor())
    };
    let base = f64::from(base) / 100.0;
    let divisor = f64::from(divisor) / 100.0;
    (base + (depth as f64).ln() * (number as f64).ln() / divisor) as i32
}

/// O que, além da profundidade e do número do lance, muda a redução de um lance tardio.
#[derive(Clone, Copy, Debug)]
struct LmrContext {
    /// Captura, promoção ou lance que dá xeque: reduz menos que um quieto.
    tactical: bool,
    /// Nó em que se espera um corte: se o primeiro lance não cortou, os tardios valem pouco.
    cut_node: bool,
    improving: bool,
    pv_node: bool,
    killer: bool,
}

/// Redução de um lance tardio, sem limites (quem chama prende entre 0 e a profundidade).
fn late_move_reduction(depth: i32, move_number: usize, ctx: LmrContext) -> i32 {
    let mut r = base_reduction(depth, move_number, ctx.tactical);
    r += i32::from(ctx.cut_node);
    r -= i32::from(ctx.improving);
    r -= i32::from(ctx.pv_node);
    r -= i32::from(ctx.killer);
    r
}

/// Tabela `[profundidade][número do lance]` da `lmr_formula` com os valores padrão.
fn build_lmr_table(tactical: bool) -> [[i32; 64]; 64] {
    let mut table = [[0; 64]; 64];
    for (depth, row) in table.iter_mut().enumerate() {
        for (number, cell) in row.iter_mut().enumerate() {
            *cell = lmr_formula(depth, number, tactical);
        }
    }
    table
}

/// Redução base dos quietos.
static LMR_TABLE: LazyLock<[[i32; 64]; 64]> = LazyLock::new(|| build_lmr_table(false));

/// Redução base dos táticos: bem menos que a dos quietos.
static LMR_TACTICAL_TABLE: LazyLock<[[i32; 64]; 64]> = LazyLock::new(|| build_lmr_table(true));

/// O que cada thread aprende por conta própria durante a busca (a TT é de todos).
struct ThreadTables {
    history: Box<History>,
    continuation: Box<ContinuationHistory>,
    captures: Box<CaptureHistory>,
    continuation4: Box<ContinuationHistory>,
    correction: Box<Corrections>,
}

impl ThreadTables {
    fn new() -> ThreadTables {
        ThreadTables {
            history: History::new(),
            continuation: ContinuationHistory::new(),
            captures: CaptureHistory::new(),
            continuation4: ContinuationHistory::new(),
            correction: Corrections::new(),
        }
    }

    fn clear(&mut self) {
        self.history.clear();
        self.continuation.clear();
        self.captures.clear();
        self.continuation4.clear();
        self.correction.clear();
    }
}

/// Pilha dos threads auxiliares: a mesma folga do thread principal (ver `uci`).
const HELPER_STACK_BYTES: usize = 64 * 1024 * 1024;

pub struct Searcher {
    tt: TranspositionTable,
    /// Tabelas do thread principal, que decide o lance e reporta as iterações.
    main: ThreadTables,
    /// Uma por thread auxiliar (Lazy SMP): buscam a mesma raiz e só contribuem pela TT.
    helpers: Vec<ThreadTables>,
    /// Rede neural da avaliação; sem ela, a avaliação à mão.
    network: Option<Arc<Network>>,
    /// Ligado durante o `go ponder`: o relógio é do adversário até o `ponderhit` desligá-lo.
    pondering: Arc<AtomicBool>,
}

impl Searcher {
    pub fn new(hash_megabytes: usize) -> Searcher {
        Searcher {
            tt: TranspositionTable::new(hash_megabytes),
            main: ThreadTables::new(),
            helpers: Vec::new(),
            network: None,
            pondering: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Sinal de ponder (ver `pondering`), para a UCI ligar no `go ponder` e desligar no
    /// `ponderhit`.
    pub fn ponder_signal(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.pondering)
    }

    /// Troca a avaliação: `Some` passa a usar a rede; `None` volta à avaliação à mão.
    pub fn set_network(&mut self, network: Option<Arc<Network>>) {
        self.network = network;
    }

    /// A rede em uso, se houver.
    pub fn network(&self) -> Option<&Network> {
        self.network.as_deref()
    }

    /// Número de threads da busca (pelo menos 1, o principal).
    pub fn set_threads(&mut self, threads: usize) {
        self.helpers
            .resize_with(threads.max(1) - 1, ThreadTables::new);
    }

    pub fn threads(&self) -> usize {
        self.helpers.len() + 1
    }

    pub fn resize(&mut self, hash_megabytes: usize) {
        self.tt = TranspositionTable::new(hash_megabytes);
    }

    pub fn clear(&mut self) {
        self.tt.clear();
        self.main.clear();
        for helper in &mut self.helpers {
            helper.clear();
        }
    }

    /// Busca a partir de `root`. `history` traz os hashes das posições anteriores da partida (sem
    /// a raiz), para reconhecer repetições. A primeira iteração sempre termina, mesmo com `stop`
    /// ligado, para que sempre haja um lance a devolver.
    ///
    /// Com mais de um thread, os auxiliares fazem o mesmo aprofundamento iterativo em paralelo e
    /// só compartilham a TT; param quando o principal termina.
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
        self.tt.new_search();
        let max_depth = limits.depth.unwrap_or(u32::MAX).min(MAX_PLY as u32 - 1);
        let Searcher {
            tt,
            main,
            helpers,
            network,
            pondering,
        } = self;
        let (tt, network, pondering) = (&*tt, network.as_deref(), &**pondering);
        let helpers_stop = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let workers: Vec<_> = helpers
                .iter_mut()
                .map(|tables| {
                    let helpers_stop = &helpers_stop;
                    std::thread::Builder::new()
                        .stack_size(HELPER_STACK_BYTES)
                        .spawn_scoped(scope, move || {
                            let mut state = new_state(
                                tt,
                                tables,
                                network,
                                root,
                                history,
                                limits,
                                helpers_stop,
                                pondering,
                            );
                            state.iterate_quietly(root, max_depth);
                            state.nodes
                        })
                        .expect("não conseguiu criar thread auxiliar da busca")
                })
                .collect();

            let mut state = new_state(tt, main, network, root, history, limits, stop, pondering);
            let mut best = SearchResult {
                best_move: Some(first_move),
                score: 0,
                depth: 0,
                nodes: 0,
                pv: vec![first_move],
            };
            let mut score = 0;
            let mut stability = 0;
            for depth in 1..=max_depth {
                state.root_depth = depth;
                state.seldepth = 0;
                let result = state.aspiration(root, depth as i32, score);
                if state.stopped {
                    break;
                }
                let score_drop = if depth > 1 { score - result } else { 0 };
                score = result;
                let pv = state.pv[0].clone();
                if depth > 1 && pv.first() == best.best_move.as_ref() {
                    stability += 1;
                } else {
                    stability = 0;
                }
                let time_scale = iteration_time_scale(stability, score_drop);
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
                // Enquanto pondera, sempre aprofunda: quem encerra é o `ponderhit` ou o `stop`.
                let deepen = state.clock().is_none_or(|_| {
                    should_start_iteration_scaled(state.start.elapsed(), limits, time_scale)
                });
                if !deepen || state.stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            helpers_stop.store(true, Ordering::Relaxed);
            let helper_nodes: u64 = workers
                .into_iter()
                .map(|worker| worker.join().expect("thread auxiliar da busca falhou"))
                .sum();
            best.nodes = state.nodes + helper_nodes;
            best
        })
    }

    /// Estado de uma busca do thread principal a partir de `root`; `history` como em `search`.
    #[cfg(test)]
    fn start<'a>(
        &'a mut self,
        root: &Position,
        history: &[u64],
        limits: &'a Limits,
        stop: &'a AtomicBool,
    ) -> SearchState<'a> {
        let network = self.network.as_deref();
        new_state(
            &self.tt,
            &mut self.main,
            network,
            root,
            history,
            limits,
            stop,
            &self.pondering,
        )
    }
}

/// Estado de busca de um thread: a TT e a rede são de todos; as tabelas, deste thread.
// Cada argumento é uma peça distinta do estado; agrupá-los só para o lint esconderia a origem.
#[allow(clippy::too_many_arguments)]
fn new_state<'a>(
    tt: &'a TranspositionTable,
    tables: &'a mut ThreadTables,
    network: Option<&'a Network>,
    root: &Position,
    history: &[u64],
    limits: &'a Limits,
    stop: &'a AtomicBool,
    pondering: &'a AtomicBool,
) -> SearchState<'a> {
    let mut hashes = Vec::with_capacity(history.len() + MAX_PLY + 1);
    hashes.extend_from_slice(history);
    hashes.push(root.hash());
    SearchState {
        accumulators: network.map_or_else(Vec::new, |net| {
            vec![Accumulators::new(net, root); MAX_PLY + 2]
        }),
        refresh_cache: network.map(RefreshCache::new),
        network,
        tt,
        history: &mut tables.history,
        continuation: &mut tables.continuation,
        captures: &mut tables.captures,
        continuation4: &mut tables.continuation4,
        correction: &mut tables.correction,
        killers: vec![[None, None]; MAX_PLY + 2],
        evals: vec![-INFINITY; MAX_PLY + 2],
        stop,
        pondering,
        awaiting_ponderhit: pondering.load(Ordering::Relaxed),
        limits,
        start: Instant::now(),
        clock_start: Instant::now(),
        nodes: 0,
        poll_counter: 0,
        seldepth: 0,
        stopped: false,
        root_depth: 0,
        root_index: history.len(),
        hashes,
        pv: (0..=MAX_PLY).map(|_| Vec::with_capacity(MAX_PLY)).collect(),
        after_null: vec![false; MAX_PLY + 2],
        moved: vec![None; MAX_PLY + 2],
        quiet_played: vec![None; MAX_PLY + 2],
        threats: vec![Bitboard::EMPTY; MAX_PLY + 2],
        excluded: vec![None; MAX_PLY + 2],
    }
}

/// Estado de uma busca em andamento.
struct SearchState<'a> {
    tt: &'a TranspositionTable,
    history: &'a mut History,
    continuation: &'a mut ContinuationHistory,
    captures: &'a mut CaptureHistory,
    /// Continuação pelo nosso lance de quatro meios-lances atrás.
    continuation4: &'a mut ContinuationHistory,
    correction: &'a mut Corrections,
    /// Até dois lances quietos que causaram corte em cada nível.
    killers: Vec<[Option<Move>; 2]>,
    /// Avaliação estática de cada nível do caminho atual (`-INFINITY` quando em xeque).
    evals: Vec<i32>,
    stop: &'a AtomicBool,
    /// Sinal de ponder do `Searcher`; `awaiting_ponderhit` diz se esta busca começou pondering e
    /// o relógio ainda não começou a contar.
    pondering: &'a AtomicBool,
    awaiting_ponderhit: bool,
    limits: &'a Limits,
    /// Começo da busca (do `go`, mesmo que pondering): mede o limite suave.
    start: Instant,
    /// Quando o nosso relógio começou a correr: o começo da busca ou o `ponderhit`. Mede o limite
    /// duro.
    clock_start: Instant,
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
    /// `moved[ply]`: lance jogado nesse nível no caminho atual (`None` para o lance nulo).
    moved: Vec<Option<PieceTo>>,
    /// `quiet_played[ply]`: o lance desse nível, quando quieto (recebe bônus se o filho falhar
    /// baixo).
    quiet_played: Vec<Option<Move>>,
    /// `threats[ply]`: casas atacadas pelo adversário na posição desse nível.
    threats: Vec<Bitboard>,
    /// `excluded[ply]`: lance que a busca desse nível ignora (a busca singular testa se as
    /// alternativas ao lance da TT chegam perto dele).
    excluded: Vec<Option<Move>>,
    network: Option<&'a Network>,
    /// `accumulators[ply]`: camada oculta da rede na posição desse nível (vazio sem rede).
    accumulators: Vec<Accumulators>,
    /// Cache de recálculo da rede para quando um rei troca de bucket (vazio sem rede).
    refresh_cache: Option<RefreshCache>,
}

impl SearchState<'_> {
    /// Aprofundamento iterativo de um thread auxiliar: busca até `max_depth` ou até mandarem
    /// parar, sem reportar; o que acha chega ao principal pela TT.
    fn iterate_quietly(&mut self, root: &Position, max_depth: u32) {
        let mut score = 0;
        for depth in 1..=max_depth {
            self.root_depth = depth;
            let result = self.aspiration(root, depth as i32, score);
            if self.stopped {
                break;
            }
            score = result;
        }
    }

    /// Janela estreita em volta da pontuação da iteração anterior, alargada a cada falha.
    fn aspiration(&mut self, root: &Position, depth: i32, previous: i32) -> i32 {
        if depth < tune::aspiration_min_depth() {
            return self.negamax(root, depth, -INFINITY, INFINITY, 0, true, false);
        }
        let mut delta = tune::aspiration_delta();
        let mut alpha = (previous - delta).max(-INFINITY);
        let mut beta = (previous + delta).min(INFINITY);
        loop {
            let score = self.negamax(root, depth, alpha, beta, 0, true, false);
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
            if delta > tune::aspiration_max_delta() {
                alpha = -INFINITY;
                beta = INFINITY;
            }
        }
    }

    /// Tempo gasto no nosso relógio, para o limite duro; `None` enquanto pondera, porque o relógio
    /// que corre é o do adversário. Na primeira consulta depois do `ponderhit` o nosso começa a
    /// contar, e o tempo pensado no ponder conta como gasto para o limite suave (a resposta veio
    /// como previsto): se ele já passou, joga na hora.
    fn clock(&mut self) -> Option<Duration> {
        if self.awaiting_ponderhit {
            if self.pondering.load(Ordering::Relaxed) {
                return None;
            }
            self.awaiting_ponderhit = false;
            self.clock_start = Instant::now();
            if self
                .limits
                .soft_time
                .is_some_and(|soft| self.start.elapsed() >= soft)
            {
                self.stopped = true;
            }
        }
        Some(self.clock_start.elapsed())
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
            let hard_time = self.limits.hard_time;
            let out_of_time =
                hard_time.is_some_and(|hard| self.clock().is_some_and(|elapsed| elapsed >= hard));
            if out_of_time || self.stop.load(Ordering::Relaxed) {
                self.stopped = true;
            }
        }
        self.stopped
    }

    /// `cut_node`: tipo de nó esperado, para as reduções. A raiz e os nós PV nunca são de corte;
    /// o primeiro filho de um nó de corte é de "todos" e vice-versa; as buscas reduzidas e de
    /// janela nula dos lances tardios esperam um corte.
    // Os parâmetros são as coordenadas do nó; agrupá-los só para o lint esconderia a recursão.
    #[allow(clippy::too_many_arguments)]
    fn negamax(
        &mut self,
        pos: &Position,
        mut depth: i32,
        mut alpha: i32,
        beta: i32,
        ply: usize,
        pv_node: bool,
        cut_node: bool,
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

        let excluded = self.excluded[ply];
        let entry = self.tt.probe(pos.hash());
        let tt_move = entry.and_then(|e| e.mv);
        if let Some(entry) = entry
            && !pv_node
            && excluded.is_none()
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
            self.corrected_eval(pos, raw_eval, ply)
        };
        self.evals[ply] = static_eval;
        // A posição melhorou em relação à nossa vez anterior? Se sim, podar é mais seguro.
        let improving = !in_check && ply >= 2 && static_eval > self.evals[ply - 2];

        // Sem lance da TT a ordenação é ruim; uma busca um pouco mais rasa sai mais barata.
        if depth >= tune::iir_min_depth() && ply > 0 && tt_move.is_none() {
            depth -= 1;
        }

        if !pv_node && !in_check && ply > 0 && excluded.is_none() && beta.abs() < MATE_BOUND {
            // Reverse futility: tão acima de beta que nem uma perda de `margem` por nível muda nada.
            if depth <= tune::rfp_max_depth() && static_eval - tune::rfp_margin() * depth >= beta {
                return static_eval;
            }
            // Null move: se mesmo passando a vez a posição segura beta numa busca rasa, corta.
            // Sem peças além de peões o risco de zugzwang é alto demais.
            if !self.after_null[ply]
                && depth >= tune::nmp_min_depth()
                && static_eval >= beta
                && pos.has_non_pawn_material(us)
            {
                let reduction = tune::nmp_base()
                    + depth / tune::nmp_depth_div()
                    + ((static_eval - beta) / tune::nmp_eval_div()).min(tune::nmp_eval_max());
                let null = pos.make_null_move();
                self.hashes.push(null.hash());
                self.moved[ply] = None;
                self.quiet_played[ply] = None;
                self.push_null(ply);
                self.after_null[ply + 1] = true;
                let score = -self.negamax(
                    &null,
                    depth - 1 - reduction,
                    -beta,
                    -beta + 1,
                    ply + 1,
                    false,
                    !cut_node,
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
        let threats = pos.attacked_by(us.flip());
        self.threats[ply] = threats;
        let ordering = MoveOrdering {
            history: self.history,
            continuation: self.continuation,
            continuation4: self.continuation4,
            captures: self.captures,
            threats,
            previous: self.previous_moves(ply),
        };
        score_moves(pos, &moves, tt_move, killers, &ordering, &mut scores);

        let original_alpha = alpha;
        let mut best_score = -INFINITY;
        let mut best_move = None;
        let mut legal: usize = 0;
        let mut quiets_tried = MoveList::new();
        let mut captures_tried = MoveList::new();
        for index in 0..moves.len() {
            let mv = pick_next(&mut moves, &mut scores, index);
            if Some(mv) == excluded {
                continue;
            }
            let next = pos.make_move(mv);
            if next.is_attacked(next.king_square(us), next.side_to_move()) {
                continue;
            }
            legal += 1;
            let quiet = !is_tactical(pos, mv);
            let mut extension = 0;
            // Extensão singular: se nenhuma alternativa ao lance da TT chega perto da pontuação
            // dele numa busca mais rasa, ele é o único bom lance e merece um nível a mais. Se até
            // sem ele a busca passa de beta, há vários lances bons e o nó corta ("multi-cut").
            if let Some(entry) = entry
                && Some(mv) == tt_move
                && ply > 0
                && excluded.is_none()
                && depth >= tune::singular_min_depth()
                && ply < 2 * self.root_depth as usize
                && entry.depth >= depth - tune::singular_tt_depth_margin()
                && entry.bound != Bound::Upper
                && score_from_tt(entry.score, ply).abs() < MATE_BOUND
            {
                let singular_beta =
                    score_from_tt(entry.score, ply) - tune::singular_margin() * depth;
                self.excluded[ply] = Some(mv);
                let score = self.negamax(
                    pos,
                    (depth - 1) / 2,
                    singular_beta - 1,
                    singular_beta,
                    ply,
                    false,
                    cut_node,
                );
                self.excluded[ply] = None;
                if self.stopped {
                    return 0;
                }
                if score < singular_beta {
                    extension = 1;
                } else if singular_beta >= beta {
                    return singular_beta;
                }
            }
            let mut new_depth = depth - 1 + extension;
            let prunable = !pv_node && !in_check && best_score > -MATE_BOUND && !next.in_check();
            if prunable && depth <= tune::see_prune_max_depth() {
                // Lance que, na troca de peças na casa de destino, perde material demais para a
                // profundidade que resta.
                let margin = if quiet {
                    tune::see_quiet_margin()
                } else {
                    tune::see_capture_margin()
                };
                if see(pos, mv) < -margin * depth {
                    continue;
                }
            }
            if prunable && quiet {
                // Late move pruning: em nível raso, depois de muitos quietos, o resto quase nunca
                // presta.
                if depth <= tune::lmp_max_depth() && legal > lmp_threshold(depth, improving) {
                    continue;
                }
                // Futility: nem com uma boa folga a posição chega a alpha com um lance quieto.
                if depth <= tune::futility_max_depth()
                    && static_eval + tune::futility_base() + tune::futility_margin() * depth
                        <= alpha
                {
                    continue;
                }
            }
            self.hashes.push(next.hash());
            self.moved[ply] = pos
                .piece_at(mv.from())
                .map(|piece| PieceTo { piece, to: mv.to() });
            self.quiet_played[ply] = quiet.then_some(mv);
            self.push_move(pos, &next, mv, ply);
            let score = if legal == 1 {
                -self.negamax(
                    &next,
                    new_depth,
                    -beta,
                    -alpha,
                    ply + 1,
                    pv_node,
                    !pv_node && !cut_node,
                )
            } else {
                // Lances tardios: primeiro uma busca reduzida; se surpreender, refaz. Capturas e
                // xeques também reduzem, só que menos.
                let late = legal > if pv_node { 3 } else { 2 };
                let reduction = if depth >= tune::lmr_min_depth() && late && !in_check {
                    let ctx = LmrContext {
                        tactical: !quiet || next.in_check(),
                        cut_node,
                        improving,
                        pv_node,
                        killer: killers.contains(&Some(mv)),
                    };
                    late_move_reduction(depth, legal, ctx).clamp(0, new_depth - 1)
                } else {
                    0
                };
                let reduced_depth = new_depth - reduction;
                let mut score = -self.negamax(
                    &next,
                    reduced_depth,
                    -alpha - 1,
                    -alpha,
                    ply + 1,
                    false,
                    reduction > 0 || !cut_node,
                );
                if reduction > 0 && score > alpha {
                    // Passou com folga do melhor até aqui: vale um nível a mais; passou raspando:
                    // um a menos basta.
                    let deeper = tune::lmr_deeper_base() + tune::lmr_deeper_margin() * new_depth;
                    if score > best_score + deeper {
                        new_depth += 1;
                    } else if score < best_score + tune::lmr_shallower_margin() {
                        new_depth -= 1;
                    }
                    if new_depth > reduced_depth {
                        score = -self.negamax(
                            &next,
                            new_depth,
                            -alpha - 1,
                            -alpha,
                            ply + 1,
                            false,
                            !cut_node,
                        );
                    }
                }
                if pv_node && score > alpha && score < beta {
                    score = -self.negamax(&next, new_depth, -beta, -alpha, ply + 1, true, false);
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
                        let bonus = history_bonus(depth);
                        if quiet {
                            self.reward_quiet(pos, mv, &quiets_tried, bonus, ply);
                        }
                        reward_captures(self.captures, pos, Some(mv), &captures_tried, bonus);
                        break;
                    }
                }
            }
            if quiet {
                quiets_tried.push(mv);
            } else if captured_kind(pos, mv).is_some() {
                captures_tried.push(mv);
            }
        }
        if legal == 0 {
            // Na busca singular, sem alternativa ao lance excluído: falha baixo (ele é singular).
            if excluded.is_some() {
                return alpha;
            }
            return if in_check { -MATE + ply as i32 } else { 0 };
        }
        if excluded.is_some() {
            // Resultado sem o melhor lance: não vale para a TT nem para a correção.
            return best_score;
        }
        let bound = if best_score >= beta {
            Bound::Lower
        } else if best_score > original_alpha {
            Bound::Exact
        } else {
            Bound::Upper
        };
        if bound == Bound::Upper {
            self.reward_parent_quiet(depth, ply);
        }
        let quiet_best = best_move.is_none_or(|mv| !is_tactical(pos, mv));
        if !in_check && quiet_best && correction_applies(bound, best_score, static_eval) {
            let keys = self.correction_keys(pos, ply);
            self.correction
                .update(us, &keys, best_score - raw_eval, depth);
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

    /// Só capturas e promoções a dama, até a posição "acalmar"; em xeque, todas as evasões. Usa e
    /// alimenta a TT (profundidade 0).
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
        let entry = self.tt.probe(pos.hash());
        if let Some(entry) = entry {
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
        let tt_move = entry.and_then(|e| e.mv);
        let original_alpha = alpha;
        let in_check = pos.in_check();
        let mut best_score = if in_check {
            -INFINITY
        } else {
            let stand_pat = self.corrected_eval(pos, self.raw_eval(pos, ply), ply);
            if stand_pat >= beta {
                return stand_pat;
            }
            alpha = alpha.max(stand_pat);
            stand_pat
        };
        let mut moves = MoveList::new();
        generate_pseudo_legal(pos, &mut moves);
        let mut scores = [0i32; MAX_MOVES];
        let ordering = MoveOrdering {
            history: self.history,
            continuation: self.continuation,
            continuation4: self.continuation4,
            captures: self.captures,
            threats: Bitboard::EMPTY,
            previous: [None, None, None],
        };
        score_moves(pos, &moves, tt_move, [None, None], &ordering, &mut scores);
        let us = pos.side_to_move();
        let mut legal = 0;
        let mut best_move = None;
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
            // A correção de continuação dos nós abaixo lê o lance deste nível.
            self.moved[ply] = pos
                .piece_at(mv.from())
                .map(|piece| PieceTo { piece, to: mv.to() });
            self.push_move(pos, &next, mv, ply);
            let score = -self.quiescence(&next, -beta, -alpha, ply + 1);
            if self.stopped {
                return 0;
            }
            if score > best_score {
                best_score = score;
                if score > alpha {
                    alpha = score;
                    best_move = Some(mv);
                    if score >= beta {
                        break;
                    }
                }
            }
        }
        if in_check && legal == 0 {
            return -MATE + ply as i32;
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
            0,
            bound,
        );
        best_score
    }

    /// Avaliação estática sem correção, do ponto de vista do lado a jogar: a rede, quando há uma,
    /// ou a avaliação à mão.
    fn raw_eval(&self, pos: &Position, ply: usize) -> i32 {
        let score = match self.network {
            Some(net) => {
                let score = net.output(&self.accumulators[ply], pos);
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

    /// Prepara a camada oculta do nível seguinte, `next`, para o lance `mv`.
    fn push_move(&mut self, pos: &Position, next: &Position, mv: Move, ply: usize) {
        if let (Some(net), Some(cache)) = (self.network, self.refresh_cache.as_mut()) {
            let (done, rest) = self.accumulators.split_at_mut(ply + 1);
            done[ply].after_move(&mut rest[0], net, pos, next, mv, cache);
        }
    }

    /// O lance nulo não mexe em peça: a camada oculta é a mesma.
    fn push_null(&mut self, ply: usize) {
        if self.network.is_some() {
            self.accumulators[ply + 1] = self.accumulators[ply];
        }
    }

    /// Avaliação `raw` somada às correções, longe das pontuações de mate.
    fn corrected_eval(&self, pos: &Position, raw: i32, ply: usize) -> i32 {
        let keys = self.correction_keys(pos, ply);
        let correction = self.correction.get(pos.side_to_move(), &keys);
        (raw + correction).clamp(-MATE_BOUND + 1, MATE_BOUND - 1)
    }

    fn correction_keys(&self, pos: &Position, ply: usize) -> CorrectionKeys {
        CorrectionKeys {
            pawn: pos.pawn_hash(),
            non_pawn: Color::ALL.map(|color| pos.non_pawn_hash(color)),
            continuation: {
                let [previous, ours, _] = self.previous_moves(ply);
                continuation_key([previous, ours])
            },
        }
    }

    /// Lance quieto que causou corte: bônus no histórico, punição para os quietos que falharam
    /// antes dele, e vira killer deste nível.
    fn reward_quiet(&mut self, pos: &Position, mv: Move, tried: &MoveList, bonus: i32, ply: usize) {
        let us = pos.side_to_move();
        let threats = self.threats[ply];
        let previous = self.previous_moves(ply);
        for (other, bonus) in [(mv, bonus)]
            .into_iter()
            .chain(tried.iter().map(|m| (m, -bonus)))
        {
            if let Some(piece) = pos.piece_at(other.from()) {
                self.update_quiet_histories(us, piece, other, threats, previous, bonus);
            }
        }
        let slots = &mut self.killers[ply];
        if slots[0] != Some(mv) {
            slots[1] = slots[0];
            slots[0] = Some(mv);
        }
    }

    /// O nó `ply` falhou baixo: o lance quieto do pai que levou até aqui foi bom para ele e ganha
    /// um bônus pequeno nos históricos do pai.
    fn reward_parent_quiet(&mut self, depth: i32, ply: usize) {
        let Some(parent) = ply.checked_sub(1) else {
            return;
        };
        let (Some(mv), Some(moved)) = (self.quiet_played[parent], self.moved[parent]) else {
            return;
        };
        let bonus = (tune::parent_bonus_mul() * depth * depth).min(tune::parent_bonus_max());
        let previous = self.previous_moves(parent);
        let threats = self.threats[parent];
        self.update_quiet_histories(moved.piece.color, moved.piece, mv, threats, previous, bonus);
    }

    /// Os lances que levaram ao nó `ply`: o do adversário, o nosso antes dele e o nosso de quatro
    /// meios-lances atrás.
    fn previous_moves(&self, ply: usize) -> [Option<PieceTo>; 3] {
        let at = |back: usize| ply.checked_sub(back).and_then(|p| self.moved[p]);
        [at(1), at(2), at(4)]
    }

    /// Soma `bonus` ao lance quieto `mv` da peça `piece` (cor `us`) num nó com ameaças `threats`
    /// e lances anteriores `previous`.
    fn update_quiet_histories(
        &mut self,
        us: Color,
        piece: Piece,
        mv: Move,
        threats: Bitboard,
        previous: [Option<PieceTo>; 3],
        bonus: i32,
    ) {
        self.history.update(us, mv, threats, bonus);
        for previous in previous[..2].iter().flatten() {
            self.continuation.update(*previous, piece, mv.to(), bonus);
        }
        if let Some(previous) = previous[2] {
            self.continuation4.update(previous, piece, mv.to(), bonus);
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

/// Ordem: lance da TT, capturas boas, promoções a dama, killers, quietos pelo histórico,
/// capturas ruins. Uma captura vale a vítima mais o histórico de capturas (o atacante mais barato
/// só desempata); é boa se o SEE alcança um limite que o histórico desloca.
fn score_moves(
    pos: &Position,
    moves: &MoveList,
    tt_move: Option<Move>,
    killers: [Option<Move>; 2],
    ordering: &MoveOrdering,
    scores: &mut [i32],
) {
    for (score, mv) in scores.iter_mut().zip(moves.iter()) {
        *score = if Some(mv) == tt_move {
            1_000_000
        } else if let Some(key) = CaptureKey::of(pos, mv) {
            let history = ordering.captures.get(key);
            let order = 10
                * (SEE_VALUE[key.victim.index()] + history / tune::capture_history_order_div())
                - ORDER_VALUE[key.piece.kind.index()];
            if see(pos, mv) >= -history / tune::capture_history_see_div() {
                100_000 + order
            } else {
                -100_000 + order
            }
        } else if mv.kind() == MoveKind::Promotion(PieceType::Queen) {
            90_000
        } else if Some(mv) == killers[0] {
            80_000
        } else if Some(mv) == killers[1] {
            79_000
        } else {
            ordering.quiet_score(pos, mv)
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

    /// Liga o sinal de ponder do `searcher`, como o `go ponder`.
    fn start_pondering(searcher: &Searcher) -> Arc<AtomicBool> {
        let pondering = searcher.ponder_signal();
        pondering.store(true, Ordering::Relaxed);
        pondering
    }

    #[test]
    fn ponderhit_after_the_budget_is_spent_plays_at_once() {
        // O ponder durou mais que o limite suave: o tempo pensado conta como gasto e, no
        // ponderhit, a busca para sem esperar o limite duro (medido a partir do ponderhit).
        let limits = Limits {
            soft_time: Some(Duration::from_millis(50)),
            hard_time: Some(Duration::from_millis(10_000)),
            ..Limits::default()
        };
        let mut searcher = Searcher::new(16);
        let pondering = start_pondering(&searcher);
        let stop = AtomicBool::new(false);
        let mut state = searcher.start(&Position::startpos(), &[], &limits, &stop);
        state.root_depth = 2;
        std::thread::sleep(Duration::from_millis(100));
        assert!(!(0..2048).any(|_| state.should_stop()), "parou pondering");
        pondering.store(false, Ordering::Relaxed);
        assert!(
            (0..2048).any(|_| state.should_stop()),
            "não parou no ponderhit"
        );
    }

    #[test]
    fn a_short_ponder_keeps_searching_on_our_clock() {
        let limits = Limits {
            soft_time: Some(Duration::from_millis(5_000)),
            hard_time: Some(Duration::from_millis(10_000)),
            ..Limits::default()
        };
        let mut searcher = Searcher::new(16);
        let pondering = start_pondering(&searcher);
        let stop = AtomicBool::new(false);
        let mut state = searcher.start(&Position::startpos(), &[], &limits, &stop);
        state.root_depth = 2;
        pondering.store(false, Ordering::Relaxed);
        assert!(!(0..2048).any(|_| state.should_stop()));
    }

    #[test]
    fn a_ponder_search_starts_the_clock_at_ponderhit() {
        // Limite duro de 50 ms, mas pondera por 300 ms: o tempo só conta depois do ponderhit.
        let limits = Limits {
            hard_time: Some(Duration::from_millis(50)),
            soft_time: Some(Duration::from_millis(50)),
            ..Limits::default()
        };
        let mut searcher = Searcher::new(16);
        let pondering = searcher.ponder_signal();
        pondering.store(true, Ordering::Relaxed);
        let start = Instant::now();
        // O tempo é medido ao fim da busca: o escopo ainda espera a thread do ponderhit.
        let (result, elapsed) = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
                pondering.store(false, Ordering::Relaxed);
            });
            let stop = AtomicBool::new(false);
            let result = searcher.search(&Position::startpos(), &[], &limits, &stop, &mut |_| {});
            (result, start.elapsed())
        });
        assert!(elapsed >= Duration::from_millis(300), "{elapsed:?}");
        assert!(elapsed < Duration::from_millis(1_300), "{elapsed:?}");
        assert!(result.best_move.is_some());
    }

    #[test]
    fn stop_ends_a_ponder_search() {
        let mut searcher = Searcher::new(16);
        searcher.ponder_signal().store(true, Ordering::Relaxed);
        let stop = AtomicBool::new(false);
        let start = Instant::now();
        let (result, elapsed) = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                stop.store(true, Ordering::Relaxed);
            });
            let limits = Limits::default();
            let result = searcher.search(&Position::startpos(), &[], &limits, &stop, &mut |_| {});
            (result, start.elapsed())
        });
        assert!(elapsed < Duration::from_millis(1_000), "{elapsed:?}");
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
            history.update(Color::White, mv, Bitboard::EMPTY, 1_600);
        }
        let high = history.get(Color::White, mv, Bitboard::EMPTY);
        assert!((10_001..=tune::history_max()).contains(&high), "{high}");
        for _ in 0..1_000 {
            history.update(Color::White, mv, Bitboard::EMPTY, -1_600);
        }
        let low = history.get(Color::White, mv, Bitboard::EMPTY);
        assert!((-tune::history_max()..-10_000).contains(&low), "{low}");
        assert_eq!(history.get(Color::Black, mv, Bitboard::EMPTY), 0);
    }

    #[test]
    fn correction_follows_the_search_error_and_stays_bounded() {
        let mut correction = CorrectionHistory::new(CORRECTION_SIZE);
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
            assert!(correction.get(Color::White, pawns) <= tune::correction_max());
        }
        assert!(correction.get(Color::White, pawns) >= tune::correction_max() - 1);
        for _ in 0..300 {
            correction.update(Color::White, pawns, -5_000, 20);
            assert!(correction.get(Color::White, pawns) >= -tune::correction_max());
        }
        assert!(correction.get(Color::White, pawns) <= -tune::correction_max() + 1);
        // Outra cor e outra estrutura de peões não são afetadas.
        assert_eq!(correction.get(Color::Black, pawns), 0);
        assert_eq!(correction.get(Color::White, pawns ^ 1), 0);
    }

    #[test]
    fn deeper_searches_move_the_correction_more() {
        let mut shallow = CorrectionHistory::new(CORRECTION_SIZE);
        let mut deep = CorrectionHistory::new(CORRECTION_SIZE);
        shallow.update(Color::White, 7, 100, 1);
        deep.update(Color::White, 7, 100, 10);
        assert!(deep.get(Color::White, 7) > shallow.get(Color::White, 7));
    }

    #[test]
    fn corrections_add_each_table_with_its_weight() {
        let mut corrections = Corrections::new();
        let keys = CorrectionKeys {
            pawn: 11,
            non_pawn: [22, 33],
            continuation: Some(44),
        };
        assert_eq!(corrections.get(Color::White, &keys), 0);
        for _ in 0..300 {
            corrections.update(Color::White, &keys, 64, 15);
        }
        let near = |value: i32, expected: i32| (value - expected).abs() <= 2;
        let all = corrections.get(Color::White, &keys);
        let total = CORRECTION_WEIGHT_PAWN
            + 2 * CORRECTION_WEIGHT_NON_PAWN
            + CORRECTION_WEIGHT_CONTINUATION;
        assert!(near(all, 64 * total / CORRECTION_WEIGHT_SCALE), "{all}");
        // Cada tabela só responde à sua chave: trocar uma tira só a parte dela.
        let other_pawns = CorrectionKeys { pawn: 12, ..keys };
        let without_pawns = corrections.get(Color::White, &other_pawns);
        let pawn_part = 64 * CORRECTION_WEIGHT_PAWN / CORRECTION_WEIGHT_SCALE;
        assert!(near(all - without_pawns, pawn_part), "{without_pawns}");
        let other_white = CorrectionKeys {
            non_pawn: [23, 33],
            ..keys
        };
        let non_pawn_part = 64 * CORRECTION_WEIGHT_NON_PAWN / CORRECTION_WEIGHT_SCALE;
        let without_white = corrections.get(Color::White, &other_white);
        assert!(near(all - without_white, non_pawn_part), "{without_white}");
        // Sem lance anterior (raiz, lance nulo) a continuação não entra.
        let no_previous = CorrectionKeys {
            continuation: None,
            ..keys
        };
        let continuation_part = 64 * CORRECTION_WEIGHT_CONTINUATION / CORRECTION_WEIGHT_SCALE;
        let without_continuation = corrections.get(Color::White, &no_previous);
        assert!(
            near(all - without_continuation, continuation_part),
            "{without_continuation}"
        );
        // Atualizar sem continuação não toca na tabela de continuação.
        let mut fresh = Corrections::new();
        fresh.update(Color::White, &no_previous, 64, 15);
        assert_eq!(fresh.continuation.get(Color::White, 44), 0);
        assert_eq!(corrections.get(Color::Black, &keys), 0);
    }

    #[test]
    fn continuation_key_tells_the_previous_moves_apart() {
        let knight = piece_to("N", "f3");
        let bishop = piece_to("B", "f3");
        let reply = piece_to("n", "c6");
        assert_eq!(continuation_key([None, Some(knight)]), None);
        let keys = [
            continuation_key([Some(reply), Some(knight)]),
            continuation_key([Some(reply), Some(bishop)]),
            continuation_key([Some(reply), None]),
            continuation_key([Some(piece_to("n", "f6")), Some(knight)]),
        ];
        for (i, a) in keys.iter().enumerate() {
            let a = a.expect("há lance anterior");
            assert!(a < CONTINUATION_CORRECTION_SIZE);
            for b in &keys[i + 1..] {
                assert_ne!(Some(a), *b);
            }
        }
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

    /// Redução base de um lance quieto.
    fn lmr_reduction(depth: i32, move_number: usize) -> i32 {
        base_reduction(depth, move_number, false)
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

    /// A fórmula parametrizada (calculada na hora no build `tune`, e que monta as tabelas no
    /// build normal) dá, com os valores padrão, exatamente as fórmulas fixas de antes.
    #[test]
    fn lmr_formula_with_defaults_matches_the_fixed_formulas() {
        for depth in 0..64usize {
            for number in 0..64usize {
                let (d, n) = ((depth as f64).ln(), (number as f64).ln());
                let (quiet, tactical) = if depth == 0 || number == 0 {
                    (0, 0)
                } else {
                    ((0.75 + d * n / 2.25) as i32, (d * n / 3.5 - 0.25) as i32)
                };
                assert_eq!(lmr_formula(depth, number, false), quiet, "{depth} {number}");
                assert_eq!(
                    lmr_formula(depth, number, true),
                    tactical,
                    "{depth} {number}"
                );
                assert_eq!(LMR_TABLE[depth][number], quiet, "{depth} {number}");
                assert_eq!(
                    LMR_TACTICAL_TABLE[depth][number], tactical,
                    "{depth} {number}"
                );
            }
        }
    }

    /// Todas as combinações de contexto de um lance tardio.
    fn every_lmr_context() -> Vec<LmrContext> {
        (0..32u8)
            .map(|bits| LmrContext {
                tactical: bits & 1 != 0,
                cut_node: bits & 2 != 0,
                improving: bits & 4 != 0,
                pv_node: bits & 8 != 0,
                killer: bits & 16 != 0,
            })
            .collect()
    }

    #[test]
    fn late_move_reduction_is_monotonic_in_each_factor() {
        for ctx in every_lmr_context() {
            for depth in 1..40 {
                for number in 1..60 {
                    let r = late_move_reduction(depth, number, ctx);
                    assert!(late_move_reduction(depth + 1, number, ctx) >= r);
                    assert!(late_move_reduction(depth, number + 1, ctx) >= r);
                    let with = |change: fn(&mut LmrContext)| {
                        let mut other = ctx;
                        change(&mut other);
                        late_move_reduction(depth, number, other)
                    };
                    // Nó de corte esperado reduz mais; melhorando, PV e killer reduzem menos;
                    // capturas e xeques reduzem menos que quietos.
                    assert!(with(|c| c.cut_node = true) >= with(|c| c.cut_node = false));
                    assert!(with(|c| c.improving = true) <= with(|c| c.improving = false));
                    assert!(with(|c| c.pv_node = true) <= with(|c| c.pv_node = false));
                    assert!(with(|c| c.killer = true) <= with(|c| c.killer = false));
                    assert!(with(|c| c.tactical = true) <= with(|c| c.tactical = false));
                }
            }
        }
    }

    #[test]
    fn late_move_reduction_factors_actually_change_the_reduction() {
        let base = LmrContext {
            tactical: false,
            cut_node: false,
            improving: false,
            pv_node: false,
            killer: false,
        };
        let r = late_move_reduction(12, 20, base);
        assert!(
            late_move_reduction(
                12,
                20,
                LmrContext {
                    cut_node: true,
                    ..base
                }
            ) > r
        );
        assert!(
            late_move_reduction(
                12,
                20,
                LmrContext {
                    improving: true,
                    ..base
                }
            ) < r
        );
        // Capturas e xeques agora também são reduzidos, mas menos que os quietos.
        let tactical = late_move_reduction(
            12,
            20,
            LmrContext {
                tactical: true,
                ..base
            },
        );
        assert!(tactical >= 1 && tactical < r);
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
        history.update(Color::White, quiet("c4", "b5"), Bitboard::EMPTY, 900);
        let killer = quiet("f3", "g5");
        let continuation = ContinuationHistory::new();
        let captures = CaptureHistory::new();
        let ordering = MoveOrdering {
            history: &history,
            continuation: &continuation,
            continuation4: &continuation,
            captures: &captures,
            threats: Bitboard::EMPTY,
            previous: [None, None, None],
        };
        let mut scores = [0i32; MAX_MOVES];
        score_moves(
            &pos,
            &moves,
            None,
            [Some(killer), None],
            &ordering,
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

    fn piece_to(piece: &str, square: &str) -> PieceTo {
        PieceTo {
            piece: Piece::from_fen_char(piece.chars().next().unwrap()).unwrap(),
            to: square.parse().unwrap(),
        }
    }

    #[test]
    fn continuation_history_is_keyed_by_the_previous_move() {
        let mut continuation = ContinuationHistory::new();
        let after_e4 = piece_to("P", "e4");
        let after_d4 = piece_to("P", "d4");
        let reply = piece_to("n", "c6");
        continuation.update(after_e4, reply.piece, reply.to, 1_000);
        assert!(continuation.get(after_e4, reply.piece, reply.to) > 0);
        assert_eq!(continuation.get(after_d4, reply.piece, reply.to), 0);
        assert_eq!(
            continuation.get(after_e4, reply.piece, "f6".parse().unwrap()),
            0
        );
    }

    #[test]
    fn move_ordering_follows_the_continuation_of_the_previous_move() {
        // Depois de 1.e4, Nc6 tem histórico de continuação; Nf6 só tem um pouco de histórico
        // simples. Com o lance anterior conhecido, Nc6 vem antes; sem ele, Nf6.
        let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
            .unwrap();
        let mut moves = MoveList::new();
        generate_pseudo_legal(&pos, &mut moves);
        let mut history = History::new();
        history.update(Color::Black, quiet("g8", "f6"), Bitboard::EMPTY, 100);
        let mut continuation = ContinuationHistory::new();
        let continuation4 = ContinuationHistory::new();
        let after_e4 = piece_to("P", "e4");
        let knight = piece_to("n", "c6");
        continuation.update(after_e4, knight.piece, knight.to, 2_000);
        let captures = CaptureHistory::new();
        let order = |previous: [Option<PieceTo>; 3]| {
            let ordering = MoveOrdering {
                history: &history,
                continuation: &continuation,
                continuation4: &continuation4,
                captures: &captures,
                threats: Bitboard::EMPTY,
                previous,
            };
            let mut scores = [0i32; MAX_MOVES];
            score_moves(&pos, &moves, None, [None, None], &ordering, &mut scores);
            let score_of = |uci: &str| {
                let i = moves.iter().position(|m| m.to_uci(false) == uci).unwrap();
                scores[i]
            };
            (score_of("b8c6"), score_of("g8f6"))
        };
        let (c6, f6) = order([Some(after_e4), None, None]);
        assert!(c6 > f6, "{c6} {f6}");
        let (c6, f6) = order([None, None, None]);
        assert!(f6 > c6, "{c6} {f6}");
    }

    fn capture(pos: &Position, uci: &str) -> Move {
        let mut moves = MoveList::new();
        generate_pseudo_legal(pos, &mut moves);
        moves
            .iter()
            .find(|m| m.to_uci(false) == uci)
            .unwrap_or_else(|| panic!("{uci} não gerado"))
    }

    #[test]
    fn capture_key_sees_piece_square_victim_and_threat() {
        // Nxe5 pega um peão sem defesa; em d4 o peão é defendido pelo peão de e5.
        let pos = Position::from_fen("4k3/8/8/4p3/3p4/5N2/8/4K3 w - - 0 1").unwrap();
        let safe = CaptureKey::of(&pos, capture(&pos, "f3e5")).unwrap();
        let defended = CaptureKey::of(&pos, capture(&pos, "f3d4")).unwrap();
        assert_eq!(safe.piece, Piece::from_fen_char('N').unwrap());
        assert_eq!(safe.to, "e5".parse().unwrap());
        assert_eq!(safe.victim, PieceType::Pawn);
        assert!(!safe.threatened);
        assert!(defended.threatened);
        // Lance quieto não tem chave.
        assert!(CaptureKey::of(&pos, quiet("f3", "g5")).is_none());
        // En passant: a vítima é o peão, mesmo com a casa de destino vazia.
        let ep = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let key = CaptureKey::of(&ep, capture(&ep, "e5d6")).unwrap();
        assert_eq!(key.victim, PieceType::Pawn);
    }

    #[test]
    fn capture_history_entries_are_independent_and_bounded() {
        let pos = Position::from_fen("4k3/8/8/4p3/3p4/5N2/8/4K3 w - - 0 1").unwrap();
        let safe = CaptureKey::of(&pos, capture(&pos, "f3e5")).unwrap();
        let defended = CaptureKey::of(&pos, capture(&pos, "f3d4")).unwrap();
        let mut captures = CaptureHistory::new();
        for _ in 0..1_000 {
            captures.update(safe, 1_600);
        }
        let high = captures.get(safe);
        assert!((10_001..=tune::history_max()).contains(&high), "{high}");
        assert_eq!(captures.get(defended), 0);
        // Mesma peça, casa e vítima, mas com a casa atacada: outra entrada.
        let threatened = CaptureKey {
            threatened: true,
            ..safe
        };
        assert_eq!(captures.get(threatened), 0);
        let other_victim = CaptureKey {
            victim: PieceType::Knight,
            ..safe
        };
        assert_eq!(captures.get(other_victim), 0);
    }

    #[test]
    fn capture_cutoff_rewards_the_capture_and_punishes_earlier_ones() {
        let pos = Position::from_fen("4k3/8/8/4p3/3p4/5N2/8/4K3 w - - 0 1").unwrap();
        let best = capture(&pos, "f3e5");
        let failed = capture(&pos, "f3d4");
        let mut tried = MoveList::new();
        tried.push(failed);
        let mut captures = CaptureHistory::new();
        reward_captures(&mut captures, &pos, Some(best), &tried, 400);
        assert!(captures.get(CaptureKey::of(&pos, best).unwrap()) > 0);
        assert!(captures.get(CaptureKey::of(&pos, failed).unwrap()) < 0);
        // Corte por lance quieto: as capturas tentadas antes também perdem.
        let mut captures = CaptureHistory::new();
        reward_captures(&mut captures, &pos, None, &tried, 400);
        assert!(captures.get(CaptureKey::of(&pos, failed).unwrap()) < 0);
        assert_eq!(captures.get(CaptureKey::of(&pos, best).unwrap()), 0);
    }

    #[test]
    fn capture_history_reorders_captures_and_moves_the_see_threshold() {
        // Brancas: Nxd5 troca cavalo por cavalo (SEE 0); Rxh7 ganha um peão solto (SEE +100);
        // Bxc6 dá o bispo pelo cavalo defendido pelo peão de b7 (SEE -10); a2-a3 é quieto.
        let pos = Position::from_fen("4k3/1p5p/2n1p3/1B1n4/8/2N5/P7/4K2R w - - 0 1").unwrap();
        let mut moves = MoveList::new();
        generate_pseudo_legal(&pos, &mut moves);
        let history = History::new();
        let continuation = ContinuationHistory::new();
        let order = |captures: &CaptureHistory| {
            let ordering = MoveOrdering {
                history: &history,
                continuation: &continuation,
                continuation4: &continuation,
                captures,
                threats: Bitboard::EMPTY,
                previous: [None, None, None],
            };
            let mut scores = [0i32; MAX_MOVES];
            score_moves(&pos, &moves, None, [None, None], &ordering, &mut scores);
            let score_of = |uci: &str| {
                let i = moves.iter().position(|m| m.to_uci(false) == uci).unwrap();
                scores[i]
            };
            (
                score_of("c3d5"),
                score_of("h1h7"),
                score_of("b5c6"),
                score_of("a2a3"),
            )
        };
        // Sem histórico: vítima maior primeiro; a captura com SEE negativo vai para o fim.
        let (nxd5, rxh7, bxc6, a3) = order(&CaptureHistory::new());
        assert!(
            nxd5 > rxh7 && rxh7 > a3 && a3 > bxc6,
            "{nxd5} {rxh7} {bxc6} {a3}"
        );

        let key = |uci: &str| CaptureKey::of(&pos, capture(&pos, uci)).unwrap();
        let mut captures = CaptureHistory::new();
        for _ in 0..1_000 {
            // Rxh7 costuma cortar: passa à frente de Nxd5.
            captures.update(key("h1h7"), 1_600);
            // Nxd5 costuma falhar: mesmo com SEE 0, vira captura ruim, depois dos quietos.
            captures.update(key("c3d5"), -1_600);
        }
        // Bxc6 cortou algumas vezes: o SEE de -10 passa a ser tolerado.
        captures.update(key("b5c6"), 3_000);
        let (nxd5, rxh7, bxc6, a3) = order(&captures);
        assert!(
            rxh7 > bxc6 && bxc6 > a3 && a3 > nxd5,
            "{nxd5} {rxh7} {bxc6} {a3}"
        );
    }

    #[test]
    fn history_is_split_by_threats_on_the_from_and_to_squares() {
        let mut history = History::new();
        let mv = quiet("c3", "e4");
        let from = Bitboard::from_square("c3".parse().unwrap());
        let to = Bitboard::from_square("e4".parse().unwrap());
        history.update(Color::White, mv, from, 1_000);
        assert!(history.get(Color::White, mv, from) > 0);
        // Atacar outra casa qualquer não muda o balde; atacar a origem ou o destino muda.
        let elsewhere = Bitboard::from_square("h7".parse().unwrap());
        assert_eq!(
            history.get(Color::White, mv, elsewhere | from),
            history.get(Color::White, mv, from)
        );
        assert_eq!(history.get(Color::White, mv, Bitboard::EMPTY), 0);
        assert_eq!(history.get(Color::White, mv, to), 0);
        assert_eq!(history.get(Color::White, mv, from | to), 0);
        history.update(Color::White, mv, from | to, -1_000);
        assert!(history.get(Color::White, mv, from | to) < 0);
        assert!(history.get(Color::White, mv, from) > 0);
    }

    #[test]
    fn move_ordering_follows_our_move_two_turns_ago() {
        // A continuação de quatro meios-lances atrás (o nosso lance anterior ao anterior) pesa na
        // ordenação: com ele conhecido, Nc6 passa Nf6, que só tem histórico simples.
        let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
            .unwrap();
        let mut moves = MoveList::new();
        generate_pseudo_legal(&pos, &mut moves);
        let mut history = History::new();
        history.update(Color::Black, quiet("g8", "f6"), Bitboard::EMPTY, 100);
        let continuation = ContinuationHistory::new();
        let mut continuation4 = ContinuationHistory::new();
        let earlier = piece_to("p", "a6");
        let knight = piece_to("n", "c6");
        continuation4.update(earlier, knight.piece, knight.to, 2_000);
        let captures = CaptureHistory::new();
        let order = |previous: [Option<PieceTo>; 3]| {
            let ordering = MoveOrdering {
                history: &history,
                continuation: &continuation,
                continuation4: &continuation4,
                captures: &captures,
                threats: Bitboard::EMPTY,
                previous,
            };
            let mut scores = [0i32; MAX_MOVES];
            score_moves(&pos, &moves, None, [None, None], &ordering, &mut scores);
            let score_of = |uci: &str| {
                let i = moves.iter().position(|m| m.to_uci(false) == uci).unwrap();
                scores[i]
            };
            (score_of("b8c6"), score_of("g8f6"))
        };
        let (c6, f6) = order([None, None, Some(earlier)]);
        assert!(c6 > f6, "{c6} {f6}");
        // No lugar do lance de dois atrás, a tabela de quatro meios-lances não é consultada.
        let (c6, f6) = order([None, Some(earlier), None]);
        assert!(f6 > c6, "{c6} {f6}");
    }

    #[test]
    fn a_fail_low_rewards_the_parents_quiet_move() {
        let stop = AtomicBool::new(false);
        let limits = Limits::default();
        let mut searcher = Searcher::new(16);
        // As pretas, sem nada contra duas damas, falham baixo: o lance quieto das brancas que
        // levou até aqui (Qa2-a1) ganha bônus no histórico delas.
        let pos = Position::from_fen("4k3/8/8/8/8/8/8/QQ2K3 b - - 0 1").unwrap();
        let parent = quiet("a2", "a1");
        let run = |searcher: &mut Searcher, quiet_parent: bool, alpha: i32| {
            let mut state = searcher.start(&pos, &[], &limits, &stop);
            state.moved[0] = Some(piece_to("Q", "a1"));
            state.quiet_played[0] = quiet_parent.then_some(parent);
            state.threats[0] = Bitboard::EMPTY;
            state.negamax(&pos, 2, alpha, alpha + 1, 1, false, false);
            state.history.get(Color::White, parent, Bitboard::EMPTY)
        };
        assert!(run(&mut searcher, true, 0) > 0);
        // Sem falha baixa (as pretas passam de alpha) ou com lance anterior que não é quieto, nada.
        searcher.clear();
        assert_eq!(run(&mut searcher, true, -30_000), 0);
        searcher.clear();
        assert_eq!(run(&mut searcher, false, 0), 0);
    }

    #[test]
    fn quiescence_stores_its_result_and_trusts_the_tt() {
        let stop = AtomicBool::new(false);
        let limits = Limits::default();
        let mut searcher = Searcher::new(16);
        // A dama preta está solta: a quiescente acha Txd5 e grava o lance com profundidade 0.
        let pos = Position::from_fen("4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1").unwrap();
        let score = searcher
            .start(&pos, &[], &limits, &stop)
            .quiescence(&pos, -INFINITY, INFINITY, 0);
        assert!(score > 300, "{score}");
        let entry = searcher
            .tt
            .probe(pos.hash())
            .expect("a quiescente grava na TT");
        assert_eq!(entry.depth, 0);
        assert_eq!(entry.mv.map(|m| m.to_uci(false)), Some("d1d5".to_string()));
        assert_eq!((entry.score, entry.bound), (score, Bound::Exact));
        // Uma entrada exata de uma busca mais funda manda: a quiescente devolve o valor dela.
        searcher.tt.store(pos.hash(), None, 777, 4, Bound::Exact);
        let score = searcher
            .start(&pos, &[], &limits, &stop)
            .quiescence(&pos, -INFINITY, INFINITY, 0);
        assert_eq!(score, 777);
    }

    #[test]
    fn a_search_excluding_the_only_move_fails_low_and_leaves_the_tt_alone() {
        let stop = AtomicBool::new(false);
        let limits = Limits::default();
        let mut searcher = Searcher::new(16);
        // Xeque da dama em g2: Kxg2 é o único lance. Sem ele, não é mate: a busca falha baixo.
        let pos = Position::from_fen("k7/8/8/8/8/8/6q1/7K w - - 0 1").unwrap();
        let only = generate_legal(&pos).iter().next().unwrap();
        searcher
            .tt
            .store(pos.hash(), Some(only), 50, 20, Bound::Lower);
        let mut state = searcher.start(&pos, &[], &limits, &stop);
        state.excluded[1] = Some(only);
        let score = state.negamax(&pos, 3, -100, 100, 1, false, false);
        assert_eq!(score, -100);
        let entry = searcher.tt.probe(pos.hash()).unwrap();
        assert_eq!((entry.mv, entry.score, entry.depth), (Some(only), 50, 20));
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
    fn a_multithreaded_search_still_finds_the_mate() {
        let mut searcher = Searcher::new(16);
        searcher.set_threads(4);
        assert_eq!(searcher.threads(), 4);
        // Problema de Morphy: 1.Ra6! e mate no lance seguinte.
        let pos = Position::from_fen("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1").unwrap();
        let result = searcher.search(&pos, &[], &depth(5), &AtomicBool::new(false), &mut |_| {});
        assert_eq!(uci(result.best_move), "a1a6");
        assert_eq!(result.score, MATE - 3);
    }

    #[test]
    fn a_multithreaded_search_respects_the_clock() {
        let pos = Position::startpos();
        let limits = Limits {
            hard_time: Some(Duration::from_millis(150)),
            soft_time: Some(Duration::from_millis(150)),
            ..Limits::default()
        };
        let mut searcher = Searcher::new(16);
        searcher.set_threads(4);
        let start = Instant::now();
        let result = searcher.search(&pos, &[], &limits, &AtomicBool::new(false), &mut |_| {});
        assert!(
            start.elapsed() < Duration::from_millis(1_000),
            "{:?}",
            start.elapsed()
        );
        assert!(generate_legal(&pos).contains(result.best_move.unwrap()));
    }

    #[test]
    fn a_multithreaded_search_counts_every_thread() {
        // Com limite de nós, o principal para no limite com um ou com quatro threads, e cada
        // auxiliar sempre termina a primeira iteração: a diferença só pode vir dos auxiliares.
        // (Comparar nós no mesmo tempo oscila quando a máquina está carregada.)
        let pos = Position::startpos();
        let limits = Limits {
            nodes: Some(20_000),
            ..Limits::default()
        };
        let run = |threads: usize| {
            let mut searcher = Searcher::new(16);
            searcher.set_threads(threads);
            let result = searcher.search(&pos, &[], &limits, &AtomicBool::new(false), &mut |_| {});
            result.nodes
        };
        let (single, multi) = (run(1), run(4));
        assert!(multi > single, "{multi} nós com 4 threads, {single} com 1");
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
