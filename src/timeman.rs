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
    /// `go ponder`: busca na posição com o lance esperado do adversário; o tempo calculado dos
    /// relógios só começa a contar no `ponderhit`.
    pub ponder: bool,
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
        // No ponder, os acertos devolvem tempo (a busca joga na hora quando o ponder já cobriu o
        // limite suave); por isso ele cresce 25%. Ideia do Stockfish.
        let soft = if params.ponder {
            (soft + soft / 4).min(hard)
        } else {
            soft
        };
        limits.soft_time = ms(soft);
        limits.hard_time = ms(hard);
    }
    limits
}

/// Vale começar mais uma iteração? Só antes de 60% do limite suave: a próxima iteração costuma
/// levar mais que todas as anteriores juntas e, começada tarde, só terminaria no limite duro.
/// Sem limite suave (depth, nodes, infinite), sempre.
pub fn should_start_iteration(elapsed: Duration, limits: &Limits) -> bool {
    should_start_iteration_scaled(elapsed, limits, 1.0)
}

/// Como `should_start_iteration`, com o limite suave multiplicado por `scale` (ver
/// `iteration_time_scale`). O limite duro não muda.
pub fn should_start_iteration_scaled(elapsed: Duration, limits: &Limits, scale: f64) -> bool {
    limits
        .soft_time
        .is_none_or(|soft| elapsed < soft.mul_f64(scale) * 6 / 10)
}

/// Fator do limite suave pelo que a busca já viu: `stability` é quantas iterações seguidas o melhor
/// lance não mudou; `score_drop`, quanto a pontuação caiu desde a iteração anterior (negativo se
/// subiu). Lance novo ou pontuação caindo pedem mais tempo; lance firme há muito, menos.
pub fn iteration_time_scale(stability: u32, score_drop: i32) -> f64 {
    const BY_STABILITY: [f64; 6] = [1.6, 1.3, 1.1, 1.0, 0.9, 0.8];
    let base = BY_STABILITY[(stability as usize).min(BY_STABILITY.len() - 1)];
    let drop = 1.0 + f64::from(score_drop.clamp(0, 100)) / 200.0;
    (base * drop).clamp(0.5, 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Option<Duration> {
        Some(Duration::from_millis(n))
    }

    #[test]
    fn a_new_iteration_only_starts_before_sixty_percent_of_the_soft_limit() {
        let limits = Limits {
            soft_time: ms(1_000),
            hard_time: ms(4_000),
            ..Limits::default()
        };
        assert!(should_start_iteration(Duration::ZERO, &limits));
        assert!(should_start_iteration(Duration::from_millis(599), &limits));
        assert!(!should_start_iteration(Duration::from_millis(600), &limits));
        assert!(!should_start_iteration(
            Duration::from_millis(2_000),
            &limits
        ));
        // Sem relógio (depth, nodes, infinite) a iteração seguinte sempre começa.
        assert!(should_start_iteration(
            Duration::from_secs(3_600),
            &Limits::default()
        ));
    }

    #[test]
    fn a_stable_best_move_saves_time_and_a_changing_or_falling_one_spends_more() {
        let limits = Limits {
            soft_time: ms(1_000),
            hard_time: ms(4_000),
            ..Limits::default()
        };
        // Com o fator 1, o corte continua nos 60%.
        assert!(should_start_iteration_scaled(
            Duration::from_millis(599),
            &limits,
            1.0
        ));
        assert!(!should_start_iteration_scaled(
            Duration::from_millis(600),
            &limits,
            1.0
        ));
        assert!(should_start_iteration_scaled(
            Duration::from_millis(900),
            &limits,
            2.0
        ));
        // Lance que acabou de mudar ganha tempo; estável há várias iterações, perde.
        let changed = iteration_time_scale(0, 0);
        let steady = iteration_time_scale(3, 0);
        let settled = iteration_time_scale(8, 0);
        assert!(
            changed > steady && steady > settled,
            "{changed} {steady} {settled}"
        );
        assert_eq!(steady, 1.0);
        // Pontuação caindo desde a iteração anterior pede mais tempo; subindo, nada muda.
        assert!(iteration_time_scale(3, 50) > steady);
        assert_eq!(iteration_time_scale(3, -50), steady);
        // Nunca passa do dobro nem cai abaixo da metade.
        assert!(iteration_time_scale(0, 1_000) <= 2.0);
        assert!(iteration_time_scale(100, 0) >= 0.5);
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
    fn a_ponder_search_gets_a_quarter_more_time() {
        // Com ponder, os acertos devolvem tempo; o limite suave cresce 25% (ideia do Stockfish).
        let params = GoParams {
            wtime: Some(60_000),
            winc: Some(1_000),
            ponder: true,
            ..GoParams::default()
        };
        let limits = compute_limits(&params, Color::White, Duration::from_millis(10));
        assert_eq!(limits.soft_time, ms(3_499 * 5 / 4));
        assert_eq!(limits.hard_time, ms(14_997));
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
