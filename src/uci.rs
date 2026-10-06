//! Protocolo UCI. A thread principal lê comandos; a busca roda em outra thread, dona do
//! `Searcher` enquanto busca, para que `isready` e `stop` sejam atendidos durante a busca.

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::bench;
use crate::eval::evaluate;
use crate::movegen::{divide, generate_legal};
use crate::moves::{Move, MoveKind};
use crate::nnue::Network;
use crate::position::Position;
use crate::search::{IterationInfo, SearchResult, Searcher, uci_score};
use crate::timeman::{GoParams, compute_limits};
use crate::types::Square;

/// Pilha da thread de busca: a recursão chega a `MAX_PLY` com posição e lista de lances por nível;
/// 64 MB sobra com folga (o padrão do Windows, 1 MB, não).
const SEARCH_STACK_BYTES: usize = 64 * 1024 * 1024;

pub const DEFAULT_HASH_MB: usize = 16;
pub const MAX_HASH_MB: usize = 65_536;
pub const MAX_THREADS: usize = 256;
pub const DEFAULT_MOVE_OVERHEAD_MS: u64 = 10;

/// Saída compartilhada entre a thread principal e a da busca; cada linha é escrita e descarregada
/// inteira.
#[derive(Clone)]
pub struct Output(Arc<Mutex<Box<dyn Write + Send>>>);

impl Output {
    pub fn new(writer: impl Write + Send + 'static) -> Output {
        Output(Arc::new(Mutex::new(Box::new(writer))))
    }

    pub fn line(&self, text: &str) {
        let mut writer = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Se a GUI fechou a saída não há a quem avisar; seguir em frente é o melhor possível.
        let _ = writeln!(writer, "{text}");
        let _ = writer.flush();
    }
}

pub struct Engine {
    position: Position,
    history: Vec<u64>,
    chess960: bool,
    move_overhead: Duration,
    searcher: Option<Searcher>,
    search_thread: Option<JoinHandle<Searcher>>,
    stop: Arc<AtomicBool>,
    /// Sinal de ponder do `Searcher`: ligado no `go ponder`, desligado no `ponderhit`.
    pondering: Arc<AtomicBool>,
    out: Output,
}

impl Engine {
    pub fn new(out: Output) -> Engine {
        let mut searcher = Searcher::new(DEFAULT_HASH_MB);
        searcher.set_network(Some(crate::nnue::embedded()));
        Engine {
            position: Position::startpos(),
            history: Vec::new(),
            chess960: false,
            move_overhead: Duration::from_millis(DEFAULT_MOVE_OVERHEAD_MS),
            pondering: searcher.ponder_signal(),
            searcher: Some(searcher),
            search_thread: None,
            stop: Arc::new(AtomicBool::new(false)),
            out,
        }
    }

    /// Trata uma linha de comando. Devolve `false` quando é para encerrar.
    pub fn handle(&mut self, line: &str) -> bool {
        crate::crash::record_command(line);
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some((&command, args)) = tokens.split_first() else {
            return true;
        };
        match command {
            "uci" => self.identify(),
            "isready" => self.out.line("readyok"),
            "ucinewgame" => {
                self.finish_search();
                self.searcher_mut().clear();
                self.position = Position::startpos();
                self.history.clear();
            }
            "setoption" => self.set_option(args),
            "position" => self.set_position(args),
            "go" => self.go(args),
            "stop" => self.finish_search(),
            // O lance esperado veio: a busca em andamento segue, agora no nosso relógio.
            "ponderhit" => self.pondering.store(false, Ordering::Relaxed),
            "quit" => {
                self.finish_search();
                return false;
            }
            "d" => self.display(),
            "eval" => {
                self.finish_search();
                let pos = self.position;
                let line = match self.searcher_mut().network() {
                    Some(net) => format!(
                        "info string eval {} (network, side to move)",
                        net.evaluate(&pos)
                    ),
                    None => format!("info string eval {} (side to move)", evaluate(&pos)),
                };
                self.out.line(&line);
            }
            "bench" => {
                self.finish_search();
                let depth = args.first().and_then(|d| d.parse().ok());
                let result = bench::run(depth.unwrap_or(bench::DEFAULT_DEPTH));
                self.out.line(&bench_line(&result));
            }
            // Parâmetros da busca no formato de entrada do SPSA do OpenBench (ver `tune.rs`).
            "tune" => {
                for t in crate::tune::TUNABLES {
                    self.out.line(&crate::tune::spsa_line(t));
                }
            }
            _ => self
                .out
                .line(&format!("info string unknown command: {command}")),
        }
        true
    }

    /// Espera a busca em andamento terminar (ela escreve o próprio `bestmove`).
    pub fn wait_for_search(&mut self) {
        if let Some(handle) = self.search_thread.take() {
            self.searcher = Some(handle.join().expect("a thread de busca entrou em pânico"));
        }
    }

    /// Pede para a busca parar e espera o `bestmove`.
    fn finish_search(&mut self) {
        if self.search_thread.is_some() {
            self.stop.store(true, Ordering::Relaxed);
            self.wait_for_search();
        }
    }

    fn searcher_mut(&mut self) -> &mut Searcher {
        self.searcher
            .as_mut()
            .expect("o searcher só falta durante uma busca")
    }

    fn identify(&self) {
        let lines = [
            format!("id name Caipora {}", env!("CARGO_PKG_VERSION")),
            "id author Matheus de Carvalho Jesus and Claude Code".to_string(),
            format!("option name Hash type spin default {DEFAULT_HASH_MB} min 1 max {MAX_HASH_MB}"),
            format!("option name Threads type spin default 1 min 1 max {MAX_THREADS}"),
            format!(
                "option name Move Overhead type spin default {DEFAULT_MOVE_OVERHEAD_MS} min 0 max 5000"
            ),
            "option name UCI_Chess960 type check default false".to_string(),
            "option name Clear Hash type button".to_string(),
            "option name EvalFile type string default <embedded>".to_string(),
            "option name Ponder type check default false".to_string(),
            "option name SyzygyPath type string default <empty>".to_string(),
        ];
        for line in lines {
            self.out.line(&line);
        }
        #[cfg(feature = "tune")]
        for t in crate::tune::TUNABLES {
            self.out.line(&format!(
                "option name {} type spin default {} min {} max {}",
                t.name, t.default, t.min, t.max
            ));
        }
        self.out.line("uciok");
    }

    fn set_option(&mut self, args: &[&str]) {
        let value_at = args.iter().position(|&t| t == "value");
        let name = args[1.min(args.len())..value_at.unwrap_or(args.len())]
            .join(" ")
            .to_lowercase();
        let value = value_at.map_or(String::new(), |i| args[i + 1..].join(" "));
        match name.as_str() {
            "hash" => match value.parse::<usize>() {
                Ok(mb) => {
                    self.finish_search();
                    self.searcher_mut().resize(mb.clamp(1, MAX_HASH_MB));
                }
                Err(_) => self.out.line("info string invalid Hash value"),
            },
            "threads" => match value.parse::<usize>() {
                Ok(threads) => {
                    self.finish_search();
                    self.searcher_mut()
                        .set_threads(threads.clamp(1, MAX_THREADS));
                }
                Err(_) => self.out.line("info string invalid Threads value"),
            },
            "move overhead" => match value.parse::<u64>() {
                Ok(ms) => self.move_overhead = Duration::from_millis(ms.min(5_000)),
                Err(_) => self.out.line("info string invalid Move Overhead value"),
            },
            "uci_chess960" => self.chess960 = value.eq_ignore_ascii_case("true"),
            "clear hash" => {
                self.finish_search();
                self.searcher_mut().clear();
            }
            "evalfile" => self.load_network(&value),
            "syzygypath" => self.load_tablebases(&value),
            // Só avisa que a GUI pode mandar `go ponder`; quem liga o ponder é o próprio `go`.
            "ponder" => {}
            #[cfg(feature = "tune")]
            _ if crate::tune::TUNABLES.iter().any(|t| t.name == name) => {
                match value.parse::<i32>() {
                    Ok(v) => {
                        self.finish_search();
                        crate::tune::set(&name, v).expect("o nome acabou de ser achado");
                    }
                    Err(_) => self.out.line(&format!("info string invalid {name} value")),
                }
            }
            _ => self
                .out
                .line(&format!("info string unknown option: {name}")),
        }
    }

    /// `SyzygyPath`: pastas com tabelas `.rtbw` (separadas por `;`, ou `:` fora do Windows);
    /// vazio ou `<empty>` desliga a sondagem.
    fn load_tablebases(&mut self, paths: &str) {
        self.finish_search();
        if paths.is_empty() || paths == "<empty>" {
            self.searcher_mut().set_tablebases(None);
            return;
        }
        let tablebases = crate::syzygy::Tablebases::open(paths);
        if tablebases.is_empty() {
            self.out
                .line(&format!("info string no tablebases found in {paths}"));
            self.searcher_mut().set_tablebases(None);
            return;
        }
        self.out.line(&format!(
            "info string found {} tablebases (up to {} pieces)",
            tablebases.len(),
            tablebases.max_pieces()
        ));
        self.searcher_mut()
            .set_tablebases(Some(Arc::new(tablebases)));
    }

    /// `EvalFile`: `<embedded>` (o padrão) usa a rede do executável; `none` (ou vazio) volta à
    /// avaliação à mão; qualquer outro valor é o caminho de uma rede. Em caso de erro, avisa e
    /// mantém a avaliação que estava.
    fn load_network(&mut self, path: &str) {
        self.finish_search();
        match path {
            "<embedded>" => {
                self.searcher_mut()
                    .set_network(Some(crate::nnue::embedded()));
                return;
            }
            "" | "none" | "<empty>" => {
                self.searcher_mut().set_network(None);
                return;
            }
            _ => {}
        }
        let loaded = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| Network::from_bytes(&bytes).map_err(|e| e.to_string()));
        match loaded {
            Ok(net) => {
                self.searcher_mut().set_network(Some(Arc::new(net)));
                self.out.line(&format!("info string loaded network {path}"));
            }
            Err(error) => self
                .out
                .line(&format!("info string cannot load network {path}: {error}")),
        }
    }

    /// `position startpos|fen <FEN> [moves ...]`. Em caso de erro a posição anterior é mantida.
    fn set_position(&mut self, args: &[&str]) {
        let moves_at = args
            .iter()
            .position(|&t| t == "moves")
            .unwrap_or(args.len());
        let mut pos = match args.first() {
            Some(&"startpos") => Position::startpos(),
            Some(&"fen") => match Position::from_fen(&args[1..moves_at].join(" ")) {
                Ok(pos) => pos,
                Err(e) => {
                    self.out.line(&format!("info string invalid fen: {e}"));
                    return;
                }
            },
            _ => {
                self.out.line("info string position needs startpos or fen");
                return;
            }
        };
        let mut history = Vec::new();
        for &token in args.iter().skip(moves_at + 1) {
            let Some(mv) = find_move(&pos, token, self.chess960) else {
                self.out.line(&format!("info string illegal move: {token}"));
                return;
            };
            history.push(pos.hash());
            pos = pos.make_move(mv);
        }
        self.position = pos;
        self.history = history;
    }

    fn go(&mut self, args: &[&str]) {
        self.finish_search();
        if args.first() == Some(&"perft") {
            let depth = args.get(1).and_then(|d| d.parse().ok()).unwrap_or(1);
            self.perft(depth);
            return;
        }
        let params = parse_go(args);
        let limits = compute_limits(&params, self.position.side_to_move(), self.move_overhead);
        let mut searcher = self
            .searcher
            .take()
            .expect("o searcher só falta durante uma busca");
        let pos = self.position;
        let history = self.history.clone();
        let chess960 = self.chess960;
        let out = self.out.clone();
        self.stop.store(false, Ordering::Relaxed);
        let stop = Arc::clone(&self.stop);
        self.pondering.store(params.ponder, Ordering::Relaxed);
        let pondering = Arc::clone(&self.pondering);
        let handle = std::thread::Builder::new()
            .name("search".to_string())
            .stack_size(SEARCH_STACK_BYTES)
            .spawn(move || {
                let result = searcher.search(&pos, &history, &limits, &stop, &mut |info| {
                    out.line(&info_line(info, chess960));
                });
                // Em `go infinite` o bestmove só pode sair depois do `stop`; no ponder, depois do
                // `ponderhit` ou do `stop`, mesmo que a busca tenha acabado antes (mate, limite).
                while (params.infinite || pondering.load(Ordering::Relaxed))
                    && !stop.load(Ordering::Relaxed)
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                out.line(&bestmove_line(&result, chess960));
                searcher
            })
            .expect("não conseguiu criar a thread de busca");
        self.search_thread = Some(handle);
    }

    fn perft(&self, depth: u32) {
        let start = Instant::now();
        let mut total = 0;
        for (mv, nodes) in divide(&self.position, depth) {
            self.out
                .line(&format!("{}: {nodes}", mv.to_uci(self.chess960)));
            total += nodes;
        }
        let elapsed = start.elapsed();
        let nps = (u128::from(total) * 1_000_000 / elapsed.as_micros().max(1)) as u64;
        self.out.line("");
        self.out.line(&format!(
            "info string perft time {} ms nps {nps}",
            elapsed.as_millis()
        ));
        self.out.line(&format!("Nodes searched: {total}"));
    }

    fn display(&self) {
        for rank in (0..8).rev() {
            let row: String = (0..8)
                .map(|file| {
                    let square = Square::new(file, rank).expect("coluna e fileira em 0..8");
                    self.position
                        .piece_at(square)
                        .map_or('.', |piece| piece.fen_char())
                })
                .flat_map(|c| [' ', c])
                .collect();
            self.out.line(&format!("{}{row}", rank + 1));
        }
        self.out.line("  a b c d e f g h");
        self.out.line(&format!("Fen: {}", self.position.to_fen()));
        self.out
            .line(&format!("Key: {:016X}", self.position.hash()));
    }

    pub fn position(&self) -> &Position {
        &self.position
    }

    pub fn history(&self) -> &[u64] {
        &self.history
    }
}

/// Lê comandos até `quit` ou até a entrada fechar; nos dois casos, para a busca antes de sair.
pub fn run(input: impl BufRead, out: Output) {
    let mut engine = Engine::new(out);
    for line in input.lines() {
        let Ok(line) = line else { break };
        if !engine.handle(&line) {
            return;
        }
    }
    engine.handle("quit");
}

/// Parâmetros do `go`. Tempos negativos (algumas GUIs mandam quando o relógio estoura) viram 0;
/// `searchmoves` e `mate` são ignorados.
pub fn parse_go(tokens: &[&str]) -> GoParams {
    let mut params = GoParams::default();
    let mut i = 0;
    while i < tokens.len() {
        let number = || {
            tokens
                .get(i + 1)
                .and_then(|value| value.parse::<i64>().ok())
                .map(|value| value.max(0) as u64)
        };
        let consumed = match tokens[i] {
            "wtime" => {
                params.wtime = number();
                true
            }
            "btime" => {
                params.btime = number();
                true
            }
            "winc" => {
                params.winc = number();
                true
            }
            "binc" => {
                params.binc = number();
                true
            }
            "movestogo" => {
                params.movestogo = number();
                true
            }
            "movetime" => {
                params.movetime = number();
                true
            }
            "nodes" => {
                params.nodes = number();
                true
            }
            "depth" => {
                params.depth = number().map(|d| d.min(u64::from(u32::MAX)) as u32);
                true
            }
            "infinite" => {
                params.infinite = true;
                false
            }
            "ponder" => {
                params.ponder = true;
                false
            }
            _ => false,
        };
        i += if consumed { 2 } else { 1 };
    }
    params
}

/// Procura o lance legal com esta notação; no roque aceita as duas notações (e1g1 e e1h1),
/// preferindo a do modo atual quando houver ambiguidade no Chess960.
fn find_move(pos: &Position, token: &str, chess960: bool) -> Option<Move> {
    let legal = generate_legal(pos);
    legal
        .iter()
        .find(|mv| mv.to_uci(chess960) == token)
        .or_else(|| {
            legal
                .iter()
                .find(|mv| mv.kind() == MoveKind::Castle && mv.to_uci(!chess960) == token)
        })
}

fn info_line(info: &IterationInfo, chess960: bool) -> String {
    let millis = info.elapsed.as_millis();
    let nps = u128::from(info.nodes) * 1000 / millis.max(1);
    let pv: Vec<String> = info.pv.iter().map(|mv| mv.to_uci(chess960)).collect();
    format!(
        "info depth {} seldepth {} multipv 1 score {} nodes {} nps {nps} hashfull {} time {millis} pv {}",
        info.depth,
        info.seldepth,
        uci_score(info.score),
        info.nodes,
        info.hashfull,
        pv.join(" ")
    )
}

/// `bestmove X`, com `ponder Y` quando a variante principal traz a resposta esperada.
fn bestmove_line(result: &SearchResult, chess960: bool) -> String {
    let Some(best) = result.best_move else {
        return "bestmove 0000".to_string();
    };
    let mut line = format!("bestmove {}", best.to_uci(chess960));
    if let [first, reply, ..] = result.pv.as_slice()
        && *first == best
    {
        line.push_str(&format!(" ponder {}", reply.to_uci(chess960)));
    }
    line
}

/// Linha final do bench, no formato que o OpenBench procura (`N nodes M nps`).
pub fn bench_line(result: &bench::BenchResult) -> String {
    format!("Bench: {} nodes {} nps", result.nodes, result.nps())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::generate_legal;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Buffer {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }

        fn lines(&self) -> Vec<String> {
            self.text().lines().map(str::to_string).collect()
        }
    }

    fn engine() -> (Engine, Buffer) {
        let buffer = Buffer::default();
        (Engine::new(Output::new(buffer.clone())), buffer)
    }

    fn send(engine: &mut Engine, commands: &[&str]) {
        for command in commands {
            assert!(engine.handle(command), "{command} encerrou o motor");
        }
    }

    /// Lance e, se houver, o lance de ponder da última linha (`bestmove X [ponder Y]`).
    fn last_bestmove(buffer: &Buffer) -> (String, Option<String>) {
        let lines = buffer.lines();
        let last = lines.last().expect("sem saída");
        let tokens: Vec<&str> = last.split_whitespace().collect();
        assert_eq!(tokens.first(), Some(&"bestmove"), "última linha: {last}");
        let ponder = match tokens.get(2) {
            Some(&"ponder") => Some(tokens[3].to_string()),
            None => None,
            Some(other) => panic!("depois do lance veio {other}"),
        };
        (tokens[1].to_string(), ponder)
    }

    fn bestmove(buffer: &Buffer) -> String {
        last_bestmove(buffer).0
    }

    #[test]
    fn uci_handshake_lists_identity_and_options() {
        let (mut engine, out) = engine();
        send(&mut engine, &["uci", "isready"]);
        let text = out.text();
        assert!(text.starts_with("id name Caipora "));
        assert!(text.contains("id author "));
        for option in [
            "option name Hash type spin default 16 min 1 max 65536",
            "option name Threads type spin default 1 min 1 max 256",
            "option name Move Overhead type spin default 10 min 0 max 5000",
            "option name UCI_Chess960 type check default false",
            "option name Clear Hash type button",
            "option name EvalFile type string default <embedded>",
            "option name Ponder type check default false",
        ] {
            assert!(text.contains(option), "falta {option}");
        }
        assert_eq!(out.lines()[out.lines().len() - 2], "uciok");
        assert_eq!(out.lines().last().unwrap(), "readyok");
    }

    #[test]
    fn syzygy_path_option_loads_the_tables() {
        let (mut engine, out) = engine();
        send(&mut engine, &["uci"]);
        assert!(
            out.text()
                .contains("option name SyzygyPath type string default <empty>")
        );
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/syzygy");
        send(
            &mut engine,
            &[&format!("setoption name SyzygyPath value {path}")],
        );
        assert!(
            out.text()
                .contains("info string found 11 tablebases (up to 4 pieces)"),
            "{}",
            out.text()
        );
        send(
            &mut engine,
            &["setoption name SyzygyPath value C:/nao/existe"],
        );
        assert!(out.text().contains("info string no tablebases found"));
    }

    #[test]
    fn position_with_moves_builds_position_and_history() {
        let (mut engine, _) = engine();
        send(&mut engine, &["position startpos moves e2e4 e7e5 g1f3"]);
        assert_eq!(
            engine.position().to_fen(),
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2"
        );
        assert_eq!(engine.history().len(), 3);
        assert_eq!(engine.history()[0], Position::startpos().hash());
    }

    #[test]
    fn castling_is_accepted_in_both_notations() {
        let kiwipete = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
        for castle in ["e1g1", "e1h1"] {
            let (mut engine, _) = engine();
            send(
                &mut engine,
                &[&format!("position fen {kiwipete} moves {castle}")],
            );
            assert_eq!(
                engine.position().to_fen(),
                "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R4RK1 b kq - 1 1"
            );
        }
    }

    #[test]
    fn invalid_position_is_reported_and_ignored() {
        let (mut engine, out) = engine();
        send(
            &mut engine,
            &[
                "position startpos moves e2e4",
                "position fen banana w - - 0 1",
            ],
        );
        assert!(out.text().contains("info string"));
        assert_eq!(engine.history().len(), 1);
        send(&mut engine, &["position startpos moves e2e5"]);
        assert!(out.lines().last().unwrap().starts_with("info string"));
    }

    #[test]
    fn go_depth_prints_info_and_a_legal_bestmove() {
        let (mut engine, out) = engine();
        send(&mut engine, &["position startpos moves e2e4", "go depth 4"]);
        engine.wait_for_search();
        let lines = out.lines();
        assert!(
            lines.iter().any(|l| l.starts_with("info depth 4 ")),
            "{lines:?}"
        );
        let best = bestmove(&out);
        let legal: Vec<String> = generate_legal(engine.position())
            .iter()
            .map(|m| m.to_uci(false))
            .collect();
        assert!(legal.contains(&best), "{best}");
    }

    #[test]
    fn go_ponder_waits_for_ponderhit_and_then_plays_on_our_clock() {
        let (mut engine, out) = engine();
        send(
            &mut engine,
            &[
                "setoption name Ponder value true",
                "position startpos moves e2e4 e7e5",
                "go ponder wtime 2000 btime 2000 winc 0 binc 0",
            ],
        );
        // Bem mais que o tempo de um lance com 2 s no relógio: pondering, não sai bestmove.
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            !out.text().contains("bestmove"),
            "bestmove antes do ponderhit"
        );
        assert!(!out.text().contains("unknown option"));
        let hit = Instant::now();
        send(&mut engine, &["ponderhit"]);
        engine.wait_for_search();
        assert!(
            hit.elapsed() < Duration::from_millis(1_000),
            "{:?}",
            hit.elapsed()
        );
        let (best, _) = last_bestmove(&out);
        assert!(!best.is_empty());
    }

    #[test]
    fn stop_ends_a_ponder_search() {
        let (mut engine, out) = engine();
        send(
            &mut engine,
            &["position startpos", "go ponder wtime 60000 btime 60000"],
        );
        std::thread::sleep(Duration::from_millis(50));
        send(&mut engine, &["stop"]);
        engine.wait_for_search();
        assert!(!bestmove(&out).is_empty());
    }

    #[test]
    fn bestmove_names_the_expected_reply_to_ponder_on() {
        let (mut engine, out) = engine();
        send(&mut engine, &["position startpos moves d2d4", "go depth 6"]);
        engine.wait_for_search();
        let (best, ponder) = last_bestmove(&out);
        let ponder = ponder.expect("sem lance de ponder");
        send(
            &mut engine,
            &[&format!("position startpos moves d2d4 {best}")],
        );
        let legal: Vec<String> = generate_legal(engine.position())
            .iter()
            .map(|mv| mv.to_uci(false))
            .collect();
        assert!(
            legal.contains(&ponder),
            "{ponder} não é legal depois de {best}"
        );
    }

    #[test]
    fn stop_ends_an_infinite_search() {
        let (mut engine, out) = engine();
        send(&mut engine, &["position startpos", "go infinite"]);
        std::thread::sleep(Duration::from_millis(50));
        send(&mut engine, &["isready"]);
        assert!(out.text().contains("readyok"));
        send(&mut engine, &["stop"]);
        engine.wait_for_search();
        assert!(!bestmove(&out).is_empty());
    }

    #[test]
    fn chess960_mode_uses_king_takes_rook_notation() {
        // O roque grande (rei b1, torre a1) é o único mate em 1: a torre chega a d1 com xeque e a
        // torre de h7 fecha a sétima fileira.
        let (mut engine, out) = engine();
        send(
            &mut engine,
            &[
                "setoption name UCI_Chess960 value true",
                "position fen 2rkr3/7R/8/8/8/8/2P5/RK6 w A - 0 1",
                "go depth 3",
            ],
        );
        engine.wait_for_search();
        assert_eq!(bestmove(&out), "b1a1");
        assert!(out.text().contains("score mate 1"));
    }

    #[test]
    fn go_perft_prints_divide_and_total() {
        let (mut engine, out) = engine();
        send(&mut engine, &["position startpos", "go perft 3"]);
        let text = out.text();
        assert!(text.contains("e2e4: 600"), "{text}");
        assert!(text.contains("Nodes searched: 8902"), "{text}");
    }

    #[test]
    fn eval_file_loads_a_network_and_reports_errors() {
        let dir = std::env::temp_dir();
        let good = dir.join(format!("caipora-net-{}.nnue", std::process::id()));
        std::fs::write(&good, crate::nnue::random_network_bytes(9)).unwrap();
        let (mut loaded, buffer) = engine();
        let path = good.to_str().unwrap();
        send(
            &mut loaded,
            &[
                &format!("setoption name EvalFile value {path}"),
                "go depth 4",
            ],
        );
        loaded.wait_for_search();
        assert!(
            buffer.text().contains("info string loaded network"),
            "{}",
            buffer.text()
        );
        assert!(!bestmove(&buffer).is_empty());
        std::fs::remove_file(&good).unwrap();
        // Arquivo que não existe ou de tamanho errado: avisa e continua com o que tinha.
        let (mut missing, buffer) = engine();
        send(
            &mut missing,
            &[
                "setoption name EvalFile value C:/nao/existe.nnue",
                "go depth 2",
            ],
        );
        missing.wait_for_search();
        assert!(
            buffer.text().contains("info string cannot load network"),
            "{}",
            buffer.text()
        );
        assert!(!bestmove(&buffer).is_empty());
    }

    #[test]
    fn eval_uses_the_network_when_one_is_loaded() {
        let file = std::env::temp_dir().join(format!("caipora-eval-{}.nnue", std::process::id()));
        let bytes = crate::nnue::random_network_bytes(21);
        std::fs::write(&file, &bytes).unwrap();
        let expected = Network::from_bytes(&bytes)
            .unwrap()
            .evaluate(&Position::startpos());
        let (mut engine, buffer) = engine();
        let path = file.to_str().unwrap();
        send(
            &mut engine,
            &[&format!("setoption name EvalFile value {path}"), "eval"],
        );
        let text = buffer.text();
        assert!(
            text.contains(&format!(
                "info string eval {expected} (network, side to move)"
            )),
            "{text}"
        );
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn the_embedded_network_is_the_default_and_none_turns_it_off() {
        let (mut default, buffer) = engine();
        send(&mut default, &["eval"]);
        assert!(
            buffer.text().contains("(network, side to move)"),
            "{}",
            buffer.text()
        );
        let (mut off, buffer) = engine();
        send(&mut off, &["setoption name EvalFile value none", "eval"]);
        let text = buffer.text();
        assert!(text.contains("info string eval"), "{text}");
        assert!(!text.contains("(network"), "{text}");
        // E volta à embutida.
        send(
            &mut off,
            &["setoption name EvalFile value <embedded>", "eval"],
        );
        assert!(
            buffer.text().contains("(network, side to move)"),
            "{}",
            buffer.text()
        );
    }

    #[test]
    fn threads_option_runs_a_multithreaded_search() {
        let (mut engine, buffer) = engine();
        send(
            &mut engine,
            &["setoption name Threads value 4", "go depth 6"],
        );
        engine.wait_for_search();
        let best = bestmove(&buffer);
        let moves = generate_legal(&Position::startpos());
        assert!(
            moves.iter().any(|m| m.to_uci(false) == best),
            "{}",
            buffer.text()
        );
    }

    #[test]
    fn tune_command_prints_the_parameters_for_openbench_spsa() {
        let (mut engine, out) = engine();
        send(&mut engine, &["tune"]);
        let lines = out.lines();
        assert_eq!(lines.len(), crate::tune::TUNABLES.len());
        assert!(lines.contains(&"rfp_margin, int, 80, 30, 200, 8, 0.002".to_string()));
    }

    #[cfg(not(feature = "tune"))]
    #[test]
    fn normal_build_does_not_expose_the_tunables() {
        let (mut engine, out) = engine();
        send(&mut engine, &["uci", "setoption name rfp_margin value 90"]);
        assert!(!out.text().contains("option name rfp_margin"));
        assert!(out.text().contains("unknown option: rfp_margin"));
        assert_eq!(crate::tune::rfp_margin(), 80);
    }

    #[cfg(feature = "tune")]
    #[test]
    fn tunables_are_spin_options_that_setoption_changes() {
        let (mut engine, out) = engine();
        send(&mut engine, &["uci"]);
        assert!(
            out.text()
                .contains("option name lmr_deeper_base type spin default 40 min 0 max 120")
        );
        assert_eq!(out.lines().last().unwrap(), "uciok");
        // Parâmetro que nenhum outro teste mexe: os valores são globais e os testes, paralelos.
        send(&mut engine, &["setoption name lmr_deeper_base value 52"]);
        assert_eq!(crate::tune::lmr_deeper_base(), 52);
        send(&mut engine, &["setoption name lmr_deeper_base value abc"]);
        assert!(out.text().contains("invalid lmr_deeper_base value"));
        assert_eq!(crate::tune::lmr_deeper_base(), 52);
        send(&mut engine, &["setoption name lmr_deeper_base value 40"]);
        assert_eq!(crate::tune::lmr_deeper_base(), 40);
    }

    #[test]
    fn quit_returns_false() {
        let (mut engine, _) = engine();
        assert!(!engine.handle("quit"));
    }

    #[test]
    fn go_parameters_are_parsed() {
        let params = parse_go(&[
            "wtime",
            "60000",
            "btime",
            "-50",
            "winc",
            "1000",
            "binc",
            "0",
            "movestogo",
            "12",
        ]);
        assert_eq!(params.wtime, Some(60_000));
        assert_eq!(params.btime, Some(0));
        assert_eq!(params.winc, Some(1_000));
        assert_eq!(params.movestogo, Some(12));
        let params = parse_go(&["depth", "9", "nodes", "1000", "infinite"]);
        assert_eq!(
            (params.depth, params.nodes, params.infinite),
            (Some(9), Some(1000), true)
        );
        assert_eq!(parse_go(&["movetime", "250"]).movetime, Some(250));
    }
}
