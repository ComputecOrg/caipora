//! Gestão de tempo da v1: um limite "suave", checado entre iterações (não começar outra se já
//! passou), e um "duro", checado durante a busca (parar de qualquer jeito).

use std::time::Duration;

use crate::types::Color;

/// Parâmetros do comando UCI `go`. Tempos em milissegundos.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoParams {
    pub wtime: Option<u64>,
    pub btime: Option<u64>,
    pub winc: Option<u64>,
    pub binc: Option<u64>,
    pub movestogo: Option<u64>,
    pub movetime: Option<u64>,
    pub depth: Option<u32>,
    pub nodes: Option<u64>,
    pub infinite: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub depth: Option<u32>,
    pub nodes: Option<u64>,
    pub soft_time: Option<Duration>,
    pub hard_time: Option<Duration>,
}

pub fn compute_limits(params: &GoParams, side: Color, move_overhead: Duration) -> Limits {
    let mut limits = Limits {
        depth: params.depth,
        nodes: params.nodes,
        ..Limits::default()
    };
    if params.infinite {
        return limits;
    }
    let overhead = move_overhead.as_millis() as u64;
    let ms = |n: u64| Some(Duration::from_millis(n));
    if let Some(movetime) = params.movetime {
        let budget = movetime.saturating_sub(overhead).max(1);
        limits.soft_time = ms(budget);
        limits.hard_time = ms(budget);
        return limits;
    }
    let (time, increment) = match side {
        Color::White => (params.wtime, params.winc),
        Color::Black => (params.btime, params.binc),
    };
    if let Some(time) = time {
        let available = time.saturating_sub(overhead).max(1);
        let divisor = params.movestogo.map_or(20, |moves| moves.max(1));
        let hard = (available / 4).max(1);
        let soft = (available / divisor + increment.unwrap_or(0) / 2).clamp(1, hard);
        limits.soft_time = ms(soft);
        limits.hard_time = ms(hard);
    }
    limits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Option<Duration> {
        Some(Duration::from_millis(n))
    }

    #[test]
    fn movetime_uses_the_whole_time_minus_overhead() {
        let params = GoParams {
            movetime: Some(1000),
            ..GoParams::default()
        };
        let limits = compute_limits(&params, Color::White, Duration::from_millis(10));
        assert_eq!(limits.hard_time, ms(990));
        assert_eq!(limits.soft_time, ms(990));
    }

    #[test]
    fn clock_with_increment() {
        let params = GoParams {
            wtime: Some(60_000),
            btime: Some(1_000),
            winc: Some(1_000),
            binc: Some(0),
            ..GoParams::default()
        };
        let white = compute_limits(&params, Color::White, Duration::from_millis(10));
        // Disponível 59.990 ms: suave = 59.990/20 + 1.000/2; duro = 59.990/4.
        assert_eq!(white.soft_time, ms(3_499));
        assert_eq!(white.hard_time, ms(14_997));
        let black = compute_limits(&params, Color::Black, Duration::from_millis(10));
        assert_eq!(black.soft_time, ms(49));
        assert_eq!(black.hard_time, ms(247));
    }

    #[test]
    fn moves_to_go_divides_the_remaining_time() {
        let params = GoParams {
            wtime: Some(90_000),
            movestogo: Some(9),
            ..GoParams::default()
        };
        let limits = compute_limits(&params, Color::White, Duration::ZERO);
        assert_eq!(limits.soft_time, ms(10_000));
        assert_eq!(limits.hard_time, ms(22_500));
    }

    #[test]
    fn almost_no_time_left_still_gives_a_positive_budget() {
        let params = GoParams {
            wtime: Some(5),
            ..GoParams::default()
        };
        let limits = compute_limits(&params, Color::White, Duration::from_millis(10));
        assert_eq!(limits.soft_time, ms(1));
        assert_eq!(limits.hard_time, ms(1));
    }

    #[test]
    fn depth_nodes_and_infinite_have_no_clock() {
        let params = GoParams {
            depth: Some(9),
            nodes: Some(5_000),
            infinite: true,
            ..GoParams::default()
        };
        let limits = compute_limits(&params, Color::White, Duration::from_millis(10));
        assert_eq!(
            limits,
            Limits {
                depth: Some(9),
                nodes: Some(5_000),
                soft_time: None,
                hard_time: None,
            }
        );
    }
}
