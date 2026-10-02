//! Parâmetros da busca ajustáveis por SPSA.
//!
//! No build normal cada parâmetro é uma função `const` que devolve o valor fixo: o compilador
//! a dobra como uma constante e a busca fica idêntica (mesma assinatura do bench, mesma
//! velocidade). Com a feature `tune` (`cargo build --release --features tune`) cada um vira um
//! atômico, exposto como opção UCI `spin` com o mesmo nome (ex.: `rfp_margin`), para o SPSA
//! (`scripts/spsa.py` ou OpenBench) mexer entre partidas. O comando UCI `tune` lista os
//! parâmetros no formato de entrada do SPSA do OpenBench. O build `tune` nunca é o que joga.

/// Um parâmetro ajustável: faixa da opção UCI e passo inicial do SPSA (`c_end` do OpenBench).
pub struct Tunable {
    pub name: &'static str,
    pub default: i32,
    pub min: i32,
    pub max: i32,
    pub step: i32,
    /// Valor em uso (no build normal, sempre o padrão).
    pub value: fn() -> i32,
}

/// Taxa de aprendizado final (`r_end`) sugerida ao SPSA do OpenBench, igual para todos.
const R_END: f64 = 0.002;

/// Linha `nome, int, valor, min, max, c_end, r_end` da entrada de SPSA do OpenBench.
pub fn spsa_line(t: &Tunable) -> String {
    format!(
        "{}, int, {}, {}, {}, {}, {R_END}",
        t.name,
        (t.value)(),
        t.min,
        t.max,
        t.step
    )
}

macro_rules! tunables {
    ($($name:ident = $default:expr, $min:expr, $max:expr, $step:expr;)*) => {
        $(
            #[cfg(not(feature = "tune"))]
            #[inline(always)]
            pub const fn $name() -> i32 {
                $default
            }

            #[cfg(feature = "tune")]
            #[inline]
            pub fn $name() -> i32 {
                VALUES[Id::$name as usize].load(std::sync::atomic::Ordering::Relaxed)
            }
        )*

        /// Índice de cada parâmetro em `VALUES`.
        #[cfg(feature = "tune")]
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy)]
        enum Id {
            $($name,)*
        }

        #[cfg(feature = "tune")]
        static VALUES: [std::sync::atomic::AtomicI32; TUNABLES.len()] =
            [$(std::sync::atomic::AtomicI32::new($default),)*];

        pub const TUNABLES: &[Tunable] = &[
            $(Tunable {
                name: stringify!($name),
                default: $default,
                min: $min,
                max: $max,
                step: $step,
                value: $name,
            },)*
        ];
    };
}

// nome = padrão, mínimo, máximo, passo do SPSA.
tunables! {
    // Janela de aspiração: profundidade mínima, meia largura inicial e a partir de quanto abre.
    aspiration_min_depth = 4, 2, 8, 1;
    aspiration_delta = 25, 8, 80, 4;
    aspiration_max_delta = 1_000, 300, 3_000, 100;
    // Redução interna sem lance da TT.
    iir_min_depth = 4, 2, 8, 1;
    // Reverse futility.
    rfp_max_depth = 8, 3, 14, 1;
    rfp_margin = 80, 30, 200, 8;
    // Null move: redução = base + profundidade/div + min((eval - beta)/eval_div, eval_max).
    nmp_min_depth = 3, 1, 6, 1;
    nmp_base = 3, 1, 6, 1;
    nmp_depth_div = 3, 2, 8, 1;
    nmp_eval_div = 200, 80, 400, 20;
    nmp_eval_max = 3, 0, 6, 1;
    // Extensão singular: a entrada da TT serve se for no máximo `tt_depth_margin` mais rasa; as
    // alternativas precisam alcançar `margin` por nível abaixo da pontuação dela.
    singular_min_depth = 8, 4, 12, 1;
    singular_tt_depth_margin = 3, 1, 6, 1;
    singular_margin = 2, 1, 6, 1;
    // Poda por SEE (margem por nível).
    see_prune_max_depth = 8, 3, 14, 1;
    see_quiet_margin = 50, 15, 150, 8;
    see_capture_margin = 100, 30, 250, 10;
    // Late move pruning: limiar = base + profundidade² (metade sem `improving`).
    lmp_max_depth = 8, 3, 14, 1;
    lmp_base = 3, 0, 10, 1;
    // Futility: base + margem por nível.
    futility_max_depth = 6, 2, 12, 1;
    futility_base = 100, 0, 250, 10;
    futility_margin = 100, 30, 250, 10;
    // LMR: base/100 + ln(profundidade)·ln(lance)/(divisor/100), uma fórmula para quietos e
    // outra para táticos (capturas, promoções, xeques).
    lmr_min_depth = 3, 2, 6, 1;
    lmr_base = 75, 0, 200, 10;
    lmr_divisor = 225, 120, 400, 15;
    lmr_tactical_base = -25, -150, 100, 10;
    lmr_tactical_divisor = 350, 150, 600, 20;
    // Depois da busca reduzida que passa de alpha: um nível a mais se superar a melhor por
    // `deeper_base + deeper_margin·profundidade`, um a menos se superar por menos de `shallower`.
    lmr_deeper_base = 40, 0, 120, 6;
    lmr_deeper_margin = 4, 0, 16, 1;
    lmr_shallower_margin = 10, 0, 40, 2;
    // Históricos: teto e bônus = min(mul·profundidade², max); o do pai após falha baixa idem.
    history_max = 16_384, 4_096, 32_768, 1_024;
    history_bonus_mul = 16, 4, 48, 2;
    history_bonus_max = 1_600, 400, 4_000, 120;
    parent_bonus_mul = 6, 0, 24, 1;
    parent_bonus_max = 600, 0, 2_000, 60;
    // Histórico de capturas: peso na ordem e deslocamento do limite de SEE (histórico / div).
    capture_history_order_div = 32, 8, 128, 4;
    capture_history_see_div = 64, 16, 256, 8;
    // Maior correção da avaliação pela estrutura de peões, em centipeões.
    correction_max = 100, 30, 250, 10;
}

/// Muda o valor de um parâmetro (preso à faixa dele); `Err` se o nome não existe.
#[cfg(feature = "tune")]
pub fn set(name: &str, value: i32) -> Result<(), String> {
    let index = TUNABLES
        .iter()
        .position(|t| t.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("unknown tunable {name}"))?;
    let t = &TUNABLES[index];
    VALUES[index].store(
        value.clamp(t.min, t.max),
        std::sync::atomic::Ordering::Relaxed,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os valores que estavam fixos em `search.rs` (main c8e8e8b) antes de virarem parâmetros: o
    /// build normal tem de buscar exatamente igual (mesma assinatura do bench).
    const ORIGINAL: &[(&str, i32)] = &[
        ("aspiration_min_depth", 4),
        ("aspiration_delta", 25),
        ("aspiration_max_delta", 1_000),
        ("iir_min_depth", 4),
        ("rfp_max_depth", 8),
        ("rfp_margin", 80),
        ("nmp_min_depth", 3),
        ("nmp_base", 3),
        ("nmp_depth_div", 3),
        ("nmp_eval_div", 200),
        ("nmp_eval_max", 3),
        ("singular_min_depth", 8),
        ("singular_tt_depth_margin", 3),
        ("singular_margin", 2),
        ("see_prune_max_depth", 8),
        ("see_quiet_margin", 50),
        ("see_capture_margin", 100),
        ("lmp_max_depth", 8),
        ("lmp_base", 3),
        ("futility_max_depth", 6),
        ("futility_base", 100),
        ("futility_margin", 100),
        ("lmr_min_depth", 3),
        ("lmr_base", 75),
        ("lmr_divisor", 225),
        ("lmr_tactical_base", -25),
        ("lmr_tactical_divisor", 350),
        ("lmr_deeper_base", 40),
        ("lmr_deeper_margin", 4),
        ("lmr_shallower_margin", 10),
        ("history_max", 16_384),
        ("history_bonus_mul", 16),
        ("history_bonus_max", 1_600),
        ("parent_bonus_mul", 6),
        ("parent_bonus_max", 600),
        ("capture_history_order_div", 32),
        ("capture_history_see_div", 64),
        ("correction_max", 100),
    ];

    #[test]
    fn defaults_match_the_original_search_constants() {
        assert_eq!(TUNABLES.len(), ORIGINAL.len());
        for &(name, value) in ORIGINAL {
            let tunable = TUNABLES.iter().find(|t| t.name == name);
            assert_eq!(tunable.map(|t| t.default), Some(value), "{name}");
        }
    }

    #[cfg(not(feature = "tune"))]
    #[test]
    fn normal_build_accessors_are_the_defaults() {
        assert!(!TUNABLES.is_empty());
        for tunable in TUNABLES {
            assert_eq!((tunable.value)(), tunable.default, "{}", tunable.name);
        }
    }

    #[test]
    fn ranges_hold_the_default_and_steps_are_positive() {
        for t in TUNABLES {
            assert!(t.min <= t.default && t.default <= t.max, "{}", t.name);
            assert!(t.step > 0 && t.step <= t.max - t.min, "{}", t.name);
            assert_eq!(t.name, t.name.to_lowercase(), "{}", t.name);
        }
    }

    #[test]
    fn divisors_can_never_reach_zero() {
        for t in TUNABLES.iter().filter(|t| t.name.contains("div")) {
            assert!(t.min > 0, "{}", t.name);
        }
    }

    #[test]
    fn spsa_line_is_in_openbench_format() {
        let rfp = TUNABLES.iter().find(|t| t.name == "rfp_margin");
        assert_eq!(
            rfp.map(spsa_line).as_deref(),
            Some("rfp_margin, int, 80, 30, 200, 8, 0.002")
        );
    }

    #[cfg(feature = "tune")]
    #[test]
    fn set_changes_a_value_clamps_it_and_rejects_unknown_names() {
        // Parâmetro que nenhum outro teste mexe: os testes rodam em paralelo e os valores são
        // globais.
        let max = TUNABLES
            .iter()
            .find(|t| t.name == "nmp_eval_div")
            .unwrap()
            .max;
        assert_eq!(set("nmp_eval_div", 250), Ok(()));
        assert_eq!(nmp_eval_div(), 250);
        // Fora da faixa vai para o limite, como a GUI faria com um `spin`.
        assert_eq!(set("NMP_EVAL_DIV", 1_000_000), Ok(()));
        assert_eq!(nmp_eval_div(), max);
        assert!(set("nao_existe", 1).is_err());
        set("nmp_eval_div", 200).unwrap();
    }
}
