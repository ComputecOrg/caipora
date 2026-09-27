//! `bench`: busca de profundidade fixa num conjunto fixo de posições, com uma thread e tabela de
//! 16 MB limpa a cada posição. O total de nós é a "assinatura" determinística da busca: muda se e
//! só se o comportamento da busca ou da avaliação mudar (o OpenBench exige isso).

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use crate::position::Position;
use crate::search::Searcher;
use crate::timeman::Limits;

pub const DEFAULT_DEPTH: u32 = 12;

const STANDARD_SUITE: &str = include_str!("../tests/data/standard.epd");
const FISCHER_SUITE: &str = include_str!("../tests/data/fischer.epd");

/// Posições: as 40 primeiras da suíte de perft padrão e 8 de Chess960 (Shredder-FEN). Mudar esta
/// lista muda a assinatura do bench.
pub fn positions() -> Vec<&'static str> {
    let fens = |suite: &'static str, count: usize| {
        suite
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(count)
            .map(|line| line.split(';').next().unwrap_or(line).trim())
            .collect::<Vec<_>>()
    };
    let mut all = fens(STANDARD_SUITE, 40);
    all.extend(fens(FISCHER_SUITE, 8));
    all
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchResult {
    pub nodes: u64,
    pub elapsed: Duration,
}

impl BenchResult {
    pub fn nps(&self) -> u64 {
        let micros = self.elapsed.as_micros().max(1);
        (u128::from(self.nodes) * 1_000_000 / micros) as u64
    }
}

pub fn run(depth: u32) -> BenchResult {
    let mut searcher = Searcher::new(16);
    let limits = Limits {
        depth: Some(depth),
        ..Limits::default()
    };
    let stop = AtomicBool::new(false);
    let start = Instant::now();
    let mut nodes = 0;
    for fen in positions() {
        let pos = Position::from_fen(fen).expect("posição de bench válida");
        searcher.clear();
        nodes += searcher
            .search(&pos, &[], &limits, &stop, &mut |_| {})
            .nodes;
    }
    BenchResult {
        nodes,
        elapsed: start.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Position;

    #[test]
    fn all_bench_positions_are_valid() {
        let fens = positions();
        assert_eq!(fens.len(), 48);
        for fen in fens {
            Position::from_fen(fen).unwrap_or_else(|e| panic!("{fen}: {e}"));
        }
    }

    #[test]
    fn bench_is_deterministic() {
        let first = run(3);
        let second = run(3);
        assert!(first.nodes > 1_000);
        assert_eq!(first.nodes, second.nodes);
    }
}
