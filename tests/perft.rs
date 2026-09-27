use caipora::movegen::perft;
use caipora::position::Position;

/// Linha EPD no formato `FEN ;D1 20 ;D2 400 ...`.
fn parse_epd_line(line: &str) -> (String, Vec<(u32, u64)>) {
    let mut parts = line.split(';');
    let fen = parts.next().expect("linha sem FEN").trim().to_string();
    let counts = parts
        .map(|part| {
            let mut fields = part.split_whitespace();
            let depth = fields.next().expect("profundidade").trim_start_matches('D');
            let nodes = fields.next().expect("contagem de nós");
            (
                depth.parse().expect("profundidade"),
                nodes.parse().expect("nós"),
            )
        })
        .collect();
    (fen, counts)
}

/// Roda toda linha do arquivo em todas as profundidades com até `max_nodes` nós.
fn run_suite(text: &str, max_nodes: u64) -> usize {
    let mut checked = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (fen, counts) = parse_epd_line(line);
        let pos = Position::from_fen(&fen).unwrap_or_else(|e| panic!("{fen}: {e}"));
        for (depth, expected) in counts.into_iter().filter(|&(_, n)| n <= max_nodes) {
            assert_eq!(perft(&pos, depth), expected, "{fen} profundidade {depth}");
            checked += 1;
        }
    }
    checked
}

#[test]
fn chessprogramming_wiki_positions_at_shallow_depth() {
    let cases = [
        (
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            4,
            197_281,
        ),
        (
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -",
            3,
            97_862,
        ),
        ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", 4, 43_238),
        (
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
            3,
            9_467,
        ),
        (
            "r2q1rk1/pP1p2pp/Q4n2/bbp1p3/Np6/1B3NBn/pPPP1PPP/R3K2R b KQ - 0 1",
            3,
            9_467,
        ),
        (
            "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
            3,
            62_379,
        ),
        (
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
            3,
            89_890,
        ),
    ];
    for (fen, depth, expected) in cases {
        let pos = Position::from_fen(fen).unwrap();
        assert_eq!(perft(&pos, depth), expected, "{fen}");
    }
}

/// Suíte completa de xadrez normal. Rodar com `cargo test --release -- --ignored`.
#[test]
#[ignore = "lento em debug; roda com --release -- --ignored"]
fn standard_suite() {
    let checked = run_suite(include_str!("data/standard.epd"), 20_000_000);
    assert!(checked > 600, "poucas contagens conferidas: {checked}");
}

/// Suíte de Chess960 (Shredder-FEN). Rodar com `cargo test --release -- --ignored`.
#[test]
#[ignore = "lento em debug; roda com --release -- --ignored"]
fn fischer_suite() {
    let checked = run_suite(include_str!("data/fischer.epd"), 2_000_000);
    assert!(checked > 3_800, "poucas contagens conferidas: {checked}");
}
