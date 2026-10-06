//! Leitura das tablebases Syzygy (WDL), conferida com valores das tabelas oficiais.
//!
//! Os arquivos pequenos de 3 e 4 peças ficam em `tests/data/syzygy`. O teste contra o oráculo
//! (API de tablebase do Lichess) precisa das tabelas completas de 3 a 5 peças:
//! `SYZYGY_PATH=<pasta> cargo test --release --test syzygy -- --ignored`.

use caipora::position::Position;
use caipora::syzygy::{Tablebases, Wdl};

fn fixtures() -> Tablebases {
    Tablebases::open(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/syzygy"))
}

fn wdl(tb: &Tablebases, fen: &str) -> Option<Wdl> {
    tb.probe_wdl(&Position::from_fen(fen).unwrap())
}

#[test]
fn finds_the_fixture_tables() {
    let tb = fixtures();
    assert!(
        tb.max_pieces() >= 4,
        "achou {} peças no máximo",
        tb.max_pieces()
    );
}

#[test]
fn queen_against_king() {
    let tb = fixtures();
    // Dama a jogar sempre vence; rei sozinho a jogar perde (exemplo verificado da especificação).
    assert_eq!(wdl(&tb, "7k/8/8/8/8/8/8/1Q2K3 w - - 0 1"), Some(Wdl::Win));
    assert_eq!(wdl(&tb, "7k/8/8/8/8/8/8/Q3K3 b - - 0 1"), Some(Wdl::Loss));
    assert_eq!(wdl(&tb, "8/8/8/8/8/8/8/KQ5k b - - 0 1"), Some(Wdl::Loss));
    // Mesma coisa com as cores trocadas (a tabela é a mesma, com a posição espelhada).
    assert_eq!(wdl(&tb, "1q2k3/8/8/8/8/8/8/7K b - - 0 1"), Some(Wdl::Win));
    assert_eq!(wdl(&tb, "q3k3/8/8/8/8/8/8/7K w - - 0 1"), Some(Wdl::Loss));
}

#[test]
fn hanging_queen_and_stalemate_are_draws() {
    let tb = fixtures();
    // O rei captura a dama desprotegida: empate (a tabela pode guardar qualquer coisa aqui).
    assert_eq!(wdl(&tb, "8/8/8/8/8/8/6Qk/K7 b - - 0 1"), Some(Wdl::Draw));
    // Afogado.
    assert_eq!(wdl(&tb, "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1"), Some(Wdl::Draw));
}

#[test]
fn rook_and_minor_pieces() {
    let tb = fixtures();
    assert_eq!(wdl(&tb, "8/8/8/8/8/2k5/8/K6R w - - 0 1"), Some(Wdl::Win));
    assert_eq!(wdl(&tb, "8/8/8/8/8/2k5/8/K6R b - - 0 1"), Some(Wdl::Loss));
    assert_eq!(wdl(&tb, "8/8/3k4/8/8/2N5/8/K7 w - - 0 1"), Some(Wdl::Draw));
    assert_eq!(wdl(&tb, "8/8/3k4/8/8/2B5/8/K7 b - - 0 1"), Some(Wdl::Draw));
    assert_eq!(wdl(&tb, "8/8/8/8/8/8/8/K1k5 w - - 0 1"), Some(Wdl::Draw));
}

#[test]
fn pawn_endings() {
    let tb = fixtures();
    // Exemplo verificado: o peão de e2 vence com o rei em e1.
    assert_eq!(wdl(&tb, "8/8/8/8/8/8/4P3/4K2k w - - 0 1"), Some(Wdl::Win));
    // Peão de torre com o rei adversário na frente: empate.
    assert_eq!(wdl(&tb, "k7/8/8/8/8/8/P7/K7 w - - 0 1"), Some(Wdl::Draw));
    // Afogado com peão.
    assert_eq!(wdl(&tb, "1k6/1P6/1K6/8/8/8/8/8 b - - 0 1"), Some(Wdl::Draw));
}

#[test]
fn four_piece_tables() {
    // Posições sorteadas e conferidas com a API de tablebase do Lichess.
    let tb = fixtures();
    let cases = [
        ("8/7N/3B4/8/k7/8/8/1K6 w - - 0 1", Wdl::Win),
        ("8/8/8/8/B4N2/3K3k/8/8 b - - 0 1", Wdl::Loss),
        ("4k3/3N4/8/8/8/2B5/8/4K3 b - - 0 1", Wdl::Draw),
        ("8/8/8/8/8/5k2/p6Q/5K2 w - - 0 1", Wdl::Win),
        ("3k4/8/8/6K1/8/Q7/4p3/8 b - - 0 1", Wdl::Draw),
        ("5K2/8/2pk4/8/5Q2/8/8/8 b - - 0 1", Wdl::Loss),
        ("8/8/8/Q7/r7/k7/8/4K3 w - - 0 1", Wdl::Win),
        ("6Q1/8/8/1k6/8/6K1/7r/8 b - - 0 1", Wdl::Loss),
        ("7R/6K1/8/8/8/2R5/8/1k6 b - - 0 1", Wdl::Loss),
        ("1k6/8/8/7R/1n6/8/8/7K w - - 0 1", Wdl::Win),
        ("7k/8/8/4n3/8/K7/8/6R1 b - - 0 1", Wdl::Draw),
        ("8/8/8/8/6n1/7K/8/3k2R1 b - - 0 1", Wdl::Loss),
    ];
    for (fen, expected) in cases {
        assert_eq!(wdl(&tb, fen), Some(expected), "{fen}");
    }
}

#[test]
fn positions_outside_the_tables_are_not_probed() {
    let tb = fixtures();
    // Com roque não sonda; com peças demais também não.
    assert_eq!(wdl(&tb, "4k3/8/8/8/8/8/8/R3K3 w Q - 0 1"), None);
    assert_eq!(
        wdl(
            &tb,
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
        ),
        None
    );
}

/// Compara com o oráculo: `tests/data/syzygy-oracle.tsv` (FEN, categoria do Lichess).
#[test]
#[ignore = "precisa das tabelas completas: SYZYGY_PATH=<pasta>"]
fn matches_the_lichess_oracle() {
    let path = std::env::var("SYZYGY_PATH").expect("SYZYGY_PATH");
    let tb = Tablebases::open(&path);
    let data = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/syzygy-oracle.tsv"
    ))
    .unwrap();
    let (mut checked, mut wrong) = (0, Vec::new());
    for line in data.lines().filter(|l| !l.is_empty()) {
        let mut fields = line.split('\t');
        let (fen, category) = (fields.next().unwrap(), fields.next().unwrap());
        let expected = match category {
            "win" => Wdl::Win,
            "cursed-win" => Wdl::CursedWin,
            "draw" => Wdl::Draw,
            "blessed-loss" => Wdl::BlessedLoss,
            "loss" => Wdl::Loss,
            _ => continue,
        };
        let got = wdl(&tb, fen);
        checked += 1;
        if got != Some(expected) {
            wrong.push(format!("{fen}: esperado {expected:?}, veio {got:?}"));
        }
    }
    assert!(checked > 100, "só {checked} posições conferidas");
    assert!(
        wrong.is_empty(),
        "{} de {checked} erradas:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
