//! Testes de ponta a ponta: sobem o executável e conversam com ele por stdin/stdout, como uma GUI
//! ou o lichess-bot fariam.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

struct Engine {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
}

impl Engine {
    fn spawn() -> Engine {
        let mut child = Command::new(env!("CARGO_BIN_EXE_caipora"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("não conseguiu iniciar o executável");
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        Engine {
            child,
            stdin,
            lines,
        }
    }

    fn send(&mut self, command: &str) {
        let stdin = self.stdin.as_mut().expect("stdin já fechado");
        writeln!(stdin, "{command}").unwrap();
        stdin.flush().unwrap();
    }

    /// Espera uma linha que satisfaça `predicate`; falha se passar de `timeout`.
    fn expect(&self, predicate: impl Fn(&str) -> bool, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) if predicate(&line) => return line,
                Ok(_) => continue,
                Err(_) => panic!("tempo esgotado esperando a linha"),
            }
        }
    }

    fn wait_exit(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.success();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        panic!("o processo não terminou a tempo");
    }
}

#[test]
fn handshake_search_and_quit() {
    let mut engine = Engine::spawn();
    engine.send("uci");
    engine.expect(|l| l == "uciok", Duration::from_secs(5));
    engine.send("isready");
    engine.expect(|l| l == "readyok", Duration::from_secs(5));
    engine.send("ucinewgame");
    engine.send("position startpos moves e2e4 c7c5");
    let start = Instant::now();
    engine.send("go movetime 200");
    let best = engine.expect(|l| l.starts_with("bestmove "), Duration::from_secs(5));
    assert!(
        start.elapsed() < Duration::from_millis(1_500),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(best.split_whitespace().count(), 2, "{best}");
    engine.send("quit");
    assert!(engine.wait_exit(Duration::from_secs(5)));
}

#[test]
fn clock_based_search_answers_in_time() {
    let mut engine = Engine::spawn();
    engine.send("position startpos");
    let start = Instant::now();
    engine.send("go wtime 2000 btime 2000 winc 20 binc 20");
    engine.expect(|l| l.starts_with("bestmove "), Duration::from_secs(5));
    // Com 2 s no relógio o limite duro é 500 ms.
    assert!(
        start.elapsed() < Duration::from_millis(900),
        "{:?}",
        start.elapsed()
    );
    engine.send("quit");
    assert!(engine.wait_exit(Duration::from_secs(5)));
}

#[test]
fn exits_when_input_closes() {
    let mut engine = Engine::spawn();
    engine.send("position startpos");
    engine.send("go infinite");
    std::thread::sleep(Duration::from_millis(100));
    drop(engine.stdin.take());
    engine.expect(|l| l.starts_with("bestmove "), Duration::from_secs(5));
    assert!(engine.wait_exit(Duration::from_secs(5)));
}

#[test]
fn bench_from_the_command_line_is_deterministic() {
    let run = || {
        let output = Command::new(env!("CARGO_BIN_EXE_caipora"))
            .args(["bench", "3"])
            .output()
            .expect("não conseguiu rodar o bench");
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        let last = stdout.lines().last().unwrap_or_default().to_string();
        let fields: Vec<&str> = last.split_whitespace().collect();
        assert_eq!(fields.len(), 5, "formato inesperado: {last}");
        assert_eq!(
            (fields[0], fields[2], fields[4]),
            ("Bench:", "nodes", "nps")
        );
        fields[1].parse::<u64>().unwrap()
    };
    assert_eq!(run(), run());
}

#[test]
fn datagen_from_the_command_line_appends_positions_to_the_file() {
    let file = std::env::temp_dir().join(format!("caipora-datagen-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&file);
    let run = || {
        let status = Command::new(env!("CARGO_BIN_EXE_caipora"))
            .args(["datagen", "2", file.to_str().unwrap(), "5", "300"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("não conseguiu rodar o datagen");
        assert!(status.success());
        std::fs::read_to_string(&file).unwrap().lines().count()
    };
    let first = run();
    assert!(first > 10, "só {first} linhas");
    // Rodar de novo acrescenta, não sobrescreve.
    assert_eq!(run(), 2 * first);
    std::fs::remove_file(&file).unwrap();
}

#[test]
fn validate_prints_the_loss_of_a_network_on_positions() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let net = dir.join(format!("caipora-validate-{id}.nnue"));
    let data = dir.join(format!("caipora-validate-{id}.txt"));
    // Rede zerada (avalia 0 = 50%) e uma vitória das brancas: perda (0,5 − 1)² = 0,25.
    std::fs::write(&net, vec![0u8; caipora::nnue::NETWORK_BYTES]).unwrap();
    std::fs::write(
        &data,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1 | 0 | 1.0\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_caipora"))
        .args([
            "validate",
            net.to_str().unwrap(),
            data.to_str().unwrap(),
            "1.0",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("não conseguiu rodar o validate");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{stdout}");
    assert!(
        stdout.contains("validation loss 0.250000 over 1 positions (wdl 1)"),
        "{stdout}"
    );
    std::fs::remove_file(&net).unwrap();
    std::fs::remove_file(&data).unwrap();
}
