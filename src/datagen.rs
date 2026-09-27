//! Dados para treinar a rede neural (NNUE). Decisão D2: só partidas do próprio Caipora.
//!
//! Self-play com busca de nós fixos, a partir da posição inicial com alguns lances aleatórios.
//! Grava as posições quietas, uma por linha: `<FEN> | <pontuação> | <resultado>`, com a
//! pontuação da busca e o resultado da partida (1.0, 0.5 ou 0.0), os dois do ponto de vista das
//! brancas.

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::movegen::generate_legal;
use crate::moves::Move;
use crate::nnue::Network;
use crate::position::Position;
use crate::search::{Searcher, insufficient_material, is_tactical};
use crate::timeman::Limits;
use crate::types::Color;

/// Pontuação a partir da qual a partida é dada como ganha; posições assim não são gravadas.
pub const ADJUDICATE_WIN: i32 = 2_500;
/// Meios-lances seguidos com a mesma vantagem decisiva para dar a vitória.
const WIN_PLIES: u32 = 4;
/// Empate adjudicado: passado este meio-lance, pontuação perto de zero por `DRAW_PLIES` seguidos.
const DRAW_AFTER_PLY: u32 = 80;
const DRAW_SCORE: i32 = 10;
const DRAW_PLIES: u32 = 8;
/// Partida que chega aqui é empate.
const MAX_GAME_PLIES: u32 = 400;
/// Abertura (depois dos lances aleatórios) desequilibrada demais é descartada.
const MAX_OPENING_SCORE: i32 = 1_000;

pub struct Config {
    pub games: u64,
    /// Nós por lance.
    pub nodes: u64,
    /// Lances aleatórios no começo de cada partida, para variar as aberturas.
    pub random_plies: u32,
    pub seed: u64,
    /// Rede neural da avaliação; sem ela, a avaliação à mão.
    pub network: Option<Arc<Network>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    WhiteWin,
    Draw,
    BlackWin,
}

impl Outcome {
    fn win_for(color: Color) -> Outcome {
        match color {
            Color::White => Outcome::WhiteWin,
            Color::Black => Outcome::BlackWin,
        }
    }

    pub fn as_text(self) -> &'static str {
        match self {
            Outcome::WhiteWin => "1.0",
            Outcome::Draw => "0.5",
            Outcome::BlackWin => "0.0",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub games: u64,
    pub positions: u64,
}

/// Joga `config.games` partidas e escreve as posições em `out`. `on_game` é chamado depois de
/// cada partida.
pub fn run<W: Write>(
    config: &Config,
    out: &mut W,
    on_game: &mut dyn FnMut(&Progress),
) -> io::Result<Progress> {
    let mut searcher = Searcher::new(16);
    searcher.set_network(config.network.clone());
    let mut rng = Rng::new(config.seed);
    let mut progress = Progress::default();
    while progress.games < config.games {
        let Some((samples, outcome)) = play_game(&mut searcher, &mut rng, config) else {
            continue;
        };
        for (fen, score) in &samples {
            writeln!(out, "{fen} | {score} | {}", outcome.as_text())?;
        }
        progress.games += 1;
        progress.positions += samples.len() as u64;
        on_game(&progress);
    }
    Ok(progress)
}

/// Uma partida: as posições gravadas (FEN e pontuação das brancas) e o resultado. `None` quando a
/// abertura sorteada não serve.
fn play_game(
    searcher: &mut Searcher,
    rng: &mut Rng,
    config: &Config,
) -> Option<(Vec<(String, i32)>, Outcome)> {
    searcher.clear();
    let mut pos = Position::startpos();
    let mut history = Vec::new();
    for _ in 0..config.random_plies {
        let moves = generate_legal(&pos);
        if moves.is_empty() {
            return None;
        }
        history.push(pos.hash());
        pos = pos.make_move(moves.as_slice()[rng.below(moves.len())]);
    }
    let limits = Limits {
        nodes: Some(config.nodes),
        ..Limits::default()
    };
    let stop = AtomicBool::new(false);
    let mut samples = Vec::new();
    let (mut white_streak, mut black_streak, mut draw_streak) = (0, 0, 0);
    for ply in 0.. {
        if let Some(outcome) = game_over(&pos, &history) {
            return Some((samples, outcome));
        }
        if ply >= MAX_GAME_PLIES {
            return Some((samples, Outcome::Draw));
        }
        let result = searcher.search(&pos, &history, &limits, &stop, &mut |_| {});
        let best = result.best_move?;
        let us = pos.side_to_move();
        let white_score = if us == Color::White {
            result.score
        } else {
            -result.score
        };
        if ply == 0 && white_score.abs() > MAX_OPENING_SCORE {
            return None;
        }
        if is_recordable(&pos, best, result.score) {
            samples.push((pos.to_fen(), white_score));
        }
        white_streak = if white_score >= ADJUDICATE_WIN {
            white_streak + 1
        } else {
            0
        };
        black_streak = if white_score <= -ADJUDICATE_WIN {
            black_streak + 1
        } else {
            0
        };
        draw_streak = if ply >= DRAW_AFTER_PLY && white_score.abs() <= DRAW_SCORE {
            draw_streak + 1
        } else {
            0
        };
        if white_streak >= WIN_PLIES {
            return Some((samples, Outcome::WhiteWin));
        }
        if black_streak >= WIN_PLIES {
            return Some((samples, Outcome::BlackWin));
        }
        if draw_streak >= DRAW_PLIES {
            return Some((samples, Outcome::Draw));
        }
        history.push(pos.hash());
        pos = pos.make_move(best);
    }
    unreachable!("o laço só sai por return")
}

/// A partida acabou nesta posição? `history` traz os hashes das posições anteriores.
fn game_over(pos: &Position, history: &[u64]) -> Option<Outcome> {
    if generate_legal(pos).is_empty() {
        return Some(if pos.in_check() {
            Outcome::win_for(pos.side_to_move().flip())
        } else {
            Outcome::Draw
        });
    }
    if pos.halfmove_clock() >= 100 || insufficient_material(pos) {
        return Some(Outcome::Draw);
    }
    let key = pos.hash();
    if history.iter().filter(|&&h| h == key).count() >= 2 {
        return Some(Outcome::Draw);
    }
    None
}

/// A posição entra nos dados? Só as quietas: fora de xeque, com um lance quieto como melhor
/// lance (a avaliação estática não vê capturas pendentes) e sem vantagem já decidida.
fn is_recordable(pos: &Position, best: Move, score: i32) -> bool {
    !pos.in_check() && !is_tactical(pos, best) && score.abs() < ADJUDICATE_WIN
}

/// Perda de uma avaliação (a rede ou a feita à mão) em posições no formato do datagen, a mesma que o treino minimiza: média de
/// (sigmoide(avaliação/400) − alvo)², com alvo = wdl·resultado + (1 − wdl)·sigmoide(pontuação/400),
/// tudo do ponto de vista do lado a jogar. Devolve a perda e quantas linhas entraram (linhas que
/// não se leem são puladas); `None` se nenhuma entrou.
pub fn validation_loss(
    evaluate: &dyn Fn(&Position) -> i32,
    text: &str,
    wdl: f64,
) -> Option<(f64, usize)> {
    let sigmoid = |cp: f64| 1.0 / (1.0 + (-cp / 400.0).exp());
    let mut total = 0.0;
    let mut count = 0;
    for line in text.lines() {
        let mut fields = line.split(" | ");
        let (Some(fen), Some(score), Some(result)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (Ok(pos), Ok(score), Ok(result)) = (
            Position::from_fen(fen),
            score.trim().parse::<f64>(),
            result.trim().parse::<f64>(),
        ) else {
            continue;
        };
        let (score, result) = match pos.side_to_move() {
            Color::White => (score, result),
            Color::Black => (-score, 1.0 - result),
        };
        let target = wdl * result + (1.0 - wdl) * sigmoid(score);
        let predicted = sigmoid(f64::from(evaluate(&pos)));
        total += (predicted - target).powi(2);
        count += 1;
    }
    (count > 0).then(|| (total / count as f64, count))
}

/// xorshift64* (Vigna): sorteio reprodutível pela semente.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::generate_legal;
    use crate::position::Position;

    fn config(games: u64, seed: u64) -> Config {
        Config {
            games,
            nodes: 400,
            random_plies: 8,
            seed,
            network: None,
        }
    }

    fn generate(config: &Config) -> String {
        let mut out = Vec::new();
        run(config, &mut out, &mut |_| {}).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn find(pos: &Position, uci: &str) -> Move {
        generate_legal(pos)
            .iter()
            .find(|m| m.to_uci(false) == uci)
            .unwrap_or_else(|| panic!("{uci} não é legal"))
    }

    #[test]
    fn every_line_is_a_quiet_position_with_score_and_result() {
        let text = generate(&config(3, 1));
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.len() > 30, "só {} linhas", lines.len());
        for line in lines {
            let fields: Vec<&str> = line.split(" | ").collect();
            assert_eq!(fields.len(), 3, "{line}");
            let pos = Position::from_fen(fields[0]).expect(line);
            assert!(!pos.in_check(), "{line}");
            let score: i32 = fields[1].parse().expect(line);
            assert!(score.abs() < ADJUDICATE_WIN, "{line}");
            assert!(["1.0", "0.5", "0.0"].contains(&fields[2]), "{line}");
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_data() {
        assert_eq!(generate(&config(2, 7)), generate(&config(2, 7)));
        assert_ne!(generate(&config(2, 7)), generate(&config(2, 8)));
    }

    #[test]
    fn a_network_plays_and_scores_the_games() {
        // Mesma semente, com e sem rede: as partidas (e as pontuações) mudam, e o formato não.
        let with_net = Config {
            network: Some(std::sync::Arc::new(crate::nnue::random_network(3))),
            ..config(2, 7)
        };
        let text = generate(&with_net);
        assert_ne!(text, generate(&config(2, 7)));
        assert!(text.lines().count() > 10);
        for line in text.lines() {
            let fields: Vec<&str> = line.split(" | ").collect();
            assert_eq!(fields.len(), 3, "{line}");
            assert!(Position::from_fen(fields[0]).is_ok(), "{line}");
        }
    }

    #[test]
    fn finished_positions_have_the_right_outcome() {
        let mated = Position::from_fen("6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1").unwrap();
        let mated = mated.make_move(find(&mated, "d1d8"));
        assert_eq!(game_over(&mated, &[]), Some(Outcome::WhiteWin));
        let stalemate = Position::from_fen("k7/8/1Q6/8/8/8/8/7K b - - 0 1").unwrap();
        assert_eq!(game_over(&stalemate, &[]), Some(Outcome::Draw));
        let bare_kings = Position::from_fen("8/8/4k3/8/8/4K3/8/8 w - - 0 1").unwrap();
        assert_eq!(game_over(&bare_kings, &[]), Some(Outcome::Draw));
        let fifty = Position::from_fen("4k3/8/8/8/8/8/8/Q3K3 b - - 100 80").unwrap();
        assert_eq!(game_over(&fifty, &[]), Some(Outcome::Draw));
        let start = Position::startpos();
        assert_eq!(game_over(&start, &[]), None);
        // A mesma posição pela terceira vez.
        let twice = [start.hash(), 1, 2, 3, start.hash(), 4, 5, 6];
        assert_eq!(game_over(&start, &twice), Some(Outcome::Draw));
        assert_eq!(game_over(&start, &twice[1..]), None);
    }

    #[test]
    fn only_quiet_positions_are_kept() {
        let pos = Position::from_fen("4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1").unwrap();
        assert!(!is_recordable(&pos, find(&pos, "d1d5"), 900));
        assert!(is_recordable(&pos, find(&pos, "e1f2"), -500));
        assert!(!is_recordable(&pos, find(&pos, "e1f2"), ADJUDICATE_WIN));
        let promotion = Position::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert!(!is_recordable(&promotion, find(&promotion, "a7a8q"), 800));
        let check = Position::from_fen("4k3/8/8/8/8/8/8/4RK2 b - - 0 1").unwrap();
        assert!(!is_recordable(&check, find(&check, "e8d7"), -500));
    }

    #[test]
    fn validation_loss_matches_the_training_target() {
        // Rede zerada: avalia tudo em 0, ou seja, 50% para o lado a jogar.
        let net = crate::nnue::Network::from_bytes(&vec![0; crate::nnue::NETWORK_BYTES]).unwrap();
        let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR";
        // Só o resultado (wdl = 1): vitória das brancas, vista pelo lado a jogar.
        let white = format!("{start} w KQkq - 0 1 | 0 | 1.0\n");
        let black = format!("{start} b KQkq - 0 1 | 0 | 1.0\n");
        let with_net = |pos: &Position| net.evaluate(pos);
        let (loss, count) = validation_loss(&with_net, &white, 1.0).unwrap();
        assert_eq!(count, 1);
        assert!((loss - 0.25).abs() < 1e-9, "{loss}");
        let (loss, _) = validation_loss(&with_net, &black, 1.0).unwrap();
        assert!((loss - 0.25).abs() < 1e-9, "{loss}");
        // Só a pontuação (wdl = 0): +400 cp vira sigmoide(1) ≈ 0,731.
        let score = format!("{start} w KQkq - 0 1 | 400 | 0.5\n");
        let (loss, _) = validation_loss(&with_net, &score, 0.0).unwrap();
        let target = 1.0 / (1.0 + (-1.0f64).exp());
        assert!((loss - (0.5 - target).powi(2)).abs() < 1e-9, "{loss}");
        // Linhas estragadas são ignoradas; sem nenhuma válida não há perda.
        let (_, count) = validation_loss(&with_net, &format!("lixo\n{white}"), 1.0).unwrap();
        assert_eq!(count, 1);
        assert_eq!(validation_loss(&with_net, "lixo\n", 1.0), None);
        // Qualquer avaliação serve, inclusive a feita à mão (a linha de base).
        let hce = crate::eval::evaluate(&Position::startpos());
        let (loss, _) = validation_loss(&crate::eval::evaluate, &white, 1.0).unwrap();
        let expected = (1.0 / (1.0 + (-f64::from(hce) / 400.0).exp()) - 1.0).powi(2);
        assert!((loss - expected).abs() < 1e-9, "{loss}");
    }

    #[test]
    fn results_are_written_from_white_point_of_view() {
        assert_eq!(Outcome::WhiteWin.as_text(), "1.0");
        assert_eq!(Outcome::Draw.as_text(), "0.5");
        assert_eq!(Outcome::BlackWin.as_text(), "0.0");
    }
}
