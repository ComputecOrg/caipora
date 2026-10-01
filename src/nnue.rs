//! Avaliação por rede neural (NNUE), no formato quantizado que o treinador `bullet` salva.
//!
//! Arquitetura (768·INPUT_BUCKETS → HIDDEN)×2 → OUTPUT_BUCKETS. As entradas são peça × casa vistas
//! de cada lado, num conjunto de pesos escolhido pela casa do próprio rei (king buckets, com as
//! colunas espelhadas quando o rei está na ala do rei); a saída tem um conjunto de pesos por faixa
//! de número de peças (output buckets). A camada oculta é mantida de forma incremental: um
//! acumulador por perspectiva, atualizado lance a lance e recalculado do zero quando o rei daquele
//! lado troca de bucket ou de metade do tabuleiro. A ativação é SCReLU (x limitado a [0, QA], ao
//! quadrado). É o layout de `ChessBucketsMirrored` + `MaterialCount` do bullet.
//!
//! O arquivo guarda, em i16 little-endian e nesta ordem:
//! - os pesos da camada oculta, 768·INPUT_BUCKETS × HIDDEN, agrupados por entrada;
//! - o viés da camada oculta (HIDDEN);
//! - os pesos de saída, por bucket de saída (2·HIDDEN cada): primeiro o lado a jogar, depois o
//!   outro;
//! - o viés de saída de cada bucket.
//!
//! O arquivo é completado com zeros até um múltiplo de 64 bytes.

use std::sync::{Arc, LazyLock};

use crate::moves::{Move, MoveKind};
use crate::position::{CastleSide, Position, castle_destinations};
use crate::types::{Color, Piece, Square};

/// Neurônios da camada oculta.
pub const HIDDEN: usize = 1024;
/// Quantização da camada oculta: 1,0 vira `QA`.
const QA: i32 = 255;
/// Quantização dos pesos de saída.
const QB: i32 = 64;
/// Saída da rede (em unidades de vitória) para centipeões. A g4 foi treinada na escala dos dados
/// do Lc0 (400 no treinador); 0,70 dessa escala (280) foi a melhor em partidas contra 0,40, 0,55
/// e 1,00 (D21): as margens da busca continuam na medida para que foram escritas.
const SCALE: i32 = 280;
/// Bucket de entrada pela casa do próprio rei, vista do próprio lado, com as colunas e–h
/// espelhadas para a–d (índice = fileira·4 + coluna espelhada). Layout nosso: na primeira fileira
/// uma casa por coluna (onde o rei passa a maior parte da partida), na segunda aos pares, a terceira
/// e a quarta juntas, e da quinta em diante um só.
#[rustfmt::skip]
const KING_BUCKETS: [usize; 32] = [
    0, 1, 2, 3,
    4, 4, 5, 5,
    6, 6, 6, 6,
    6, 6, 6, 6,
    7, 7, 7, 7,
    7, 7, 7, 7,
    7, 7, 7, 7,
    7, 7, 7, 7,
];
pub const INPUT_BUCKETS: usize = 8;
/// Buckets de saída por número de peças no tabuleiro.
pub const OUTPUT_BUCKETS: usize = 8;
/// Número de valores i16 no arquivo, sem o preenchimento.
const NETWORK_VALUES: usize =
    768 * INPUT_BUCKETS * HIDDEN + HIDDEN + OUTPUT_BUCKETS * 2 * HIDDEN + OUTPUT_BUCKETS;
/// Tamanho do arquivo: os valores, completados até múltiplo de 64 bytes.
pub const NETWORK_BYTES: usize = (2 * NETWORK_VALUES).div_ceil(64) * 64;

/// Rede que vai dentro do executável (D14): treinada só com partidas do próprio Caipora (D2).
static EMBEDDED: LazyLock<Arc<Network>> = LazyLock::new(|| {
    let bytes = include_bytes!("../net/caipora-g6.nnue");
    Arc::new(Network::from_bytes(bytes).expect("a rede embutida tem o formato certo"))
});

/// A rede embutida no executável.
pub fn embedded() -> Arc<Network> {
    Arc::clone(&EMBEDDED)
}

/// Uma coluna da camada oculta: um valor por neurônio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct Accumulator {
    values: [i16; HIDDEN],
}

impl Accumulator {
    fn add(&mut self, column: &Accumulator) {
        for (value, weight) in self.values.iter_mut().zip(&column.values) {
            *value += weight;
        }
    }

    fn sub(&mut self, column: &Accumulator) {
        for (value, weight) in self.values.iter_mut().zip(&column.values) {
            *value -= weight;
        }
    }
}

pub struct Network {
    /// Uma coluna por entrada (768 por bucket de entrada).
    feature_weights: Vec<Accumulator>,
    feature_bias: Accumulator,
    /// 2·HIDDEN por bucket de saída.
    output_weights: Vec<i16>,
    output_bias: [i16; OUTPUT_BUCKETS],
}

/// Maior |peso de saída| aceito: ativação (até QA) × peso precisa caber em 16 bits.
const MAX_OUTPUT_WEIGHT: i16 = i16::MAX / QA as i16;

#[derive(Debug, PartialEq, Eq)]
pub enum NetworkError {
    /// O arquivo não tem o tamanho desta arquitetura.
    Size { expected: usize, found: usize },
    /// Peso de saída grande demais para o cálculo em 16 bits.
    OutputWeightTooLarge { index: usize, value: i16 },
}

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkError::Size { expected, found } => {
                write!(f, "{found} bytes, expected {expected}")
            }
            NetworkError::OutputWeightTooLarge { index, value } => {
                write!(
                    f,
                    "output weight {index} is {value}, limit {MAX_OUTPUT_WEIGHT}"
                )
            }
        }
    }
}

impl Network {
    /// Lê a rede do formato do arquivo; o tamanho precisa bater exatamente e os pesos de saída
    /// precisam caber no cálculo em 16 bits.
    pub fn from_bytes(bytes: &[u8]) -> Result<Network, NetworkError> {
        if bytes.len() != NETWORK_BYTES {
            return Err(NetworkError::Size {
                expected: NETWORK_BYTES,
                found: bytes.len(),
            });
        }
        let mut values = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| i16::from_le_bytes(pair));
        let mut column = || {
            let mut acc = Accumulator {
                values: [0; HIDDEN],
            };
            for (slot, value) in acc.values.iter_mut().zip(&mut values) {
                *slot = value;
            }
            acc
        };
        let feature_weights = (0..768 * INPUT_BUCKETS).map(|_| column()).collect();
        let feature_bias = column();
        let output_weights: Vec<i16> = (&mut values).take(OUTPUT_BUCKETS * 2 * HIDDEN).collect();
        let mut output_bias = [0; OUTPUT_BUCKETS];
        for (slot, value) in output_bias.iter_mut().zip(&mut values) {
            *slot = value;
        }
        if let Some((index, &value)) = output_weights
            .iter()
            .enumerate()
            .find(|(_, w)| w.unsigned_abs() > MAX_OUTPUT_WEIGHT.unsigned_abs())
        {
            return Err(NetworkError::OutputWeightTooLarge { index, value });
        }
        Ok(Network {
            feature_weights,
            feature_bias,
            output_weights,
            output_bias,
        })
    }

    /// Avaliação do zero, em centipeões, do ponto de vista do lado a jogar.
    pub fn evaluate(&self, pos: &Position) -> i32 {
        self.output(&Accumulators::new(self, pos), pos)
    }

    /// Saída da rede a partir dos acumuladores já calculados de `pos`.
    pub fn output(&self, acc: &Accumulators, pos: &Position) -> i32 {
        let bucket = output_bucket(pos.occupied().count());
        let weights = &self.output_weights[bucket * 2 * HIDDEN..(bucket + 1) * 2 * HIDDEN];
        let (us, them) = match pos.side_to_move() {
            Color::White => (&acc.white, &acc.black),
            Color::Black => (&acc.black, &acc.white),
        };
        let half = |acc: &Accumulator, weights: &[i16]| -> i32 {
            acc.values
                .iter()
                .zip(weights)
                .map(|(&value, &weight)| {
                    // v·w cabe em 16 bits (|w| ≤ MAX_OUTPUT_WEIGHT, conferido na leitura); só a
                    // segunda multiplicação vai para 32. É a forma que vira `pmaddwd` no AVX2.
                    let clipped = value.clamp(0, QA as i16);
                    i32::from(clipped * weight) * i32::from(clipped)
                })
                .sum()
        };
        let sum = half(us, &weights[..HIDDEN]) + half(them, &weights[HIDDEN..]);
        (sum / QA + i32::from(self.output_bias[bucket])) * SCALE / (QA * QB)
    }
}

/// Bucket de saída pelo número de peças (reis incluídos), como `MaterialCount` do bullet.
pub fn output_bucket(pieces: u32) -> usize {
    (pieces as usize - 2) / 32usize.div_ceil(OUTPUT_BUCKETS)
}

/// Casa do rei vista do lado `perspective` (pelas pretas, espelhada na vertical).
fn view(perspective: Color, square: usize) -> usize {
    match perspective {
        Color::White => square,
        Color::Black => square ^ 56,
    }
}

/// Bucket de entrada e espelho (0 ou 7) para o rei de `perspective` em `king`.
fn king_key(perspective: Color, king: Square) -> (usize, usize) {
    let k = view(perspective, king.index());
    let mirrored_file = [0, 1, 2, 3, 3, 2, 1, 0][k % 8];
    let flip = if k % 8 > 3 { 7 } else { 0 };
    (KING_BUCKETS[(k / 8) * 4 + mirrored_file], flip)
}

/// Índice da entrada "peça em casa" vista pelo lado `perspective`, com o rei desse lado em
/// `king`: 768 por bucket do rei; dentro dele, 0..384 para as peças do próprio lado e 384..768 para
/// as do outro; pelas pretas, o tabuleiro é espelhado na vertical, e com o rei na ala do rei, na
/// horizontal.
pub fn feature(perspective: Color, piece: Piece, square: Square, king: Square) -> usize {
    let (bucket, flip) = king_key(perspective, king);
    let side = if piece.color == perspective { 0 } else { 384 };
    768 * bucket + ((side + 64 * piece.kind.index() + view(perspective, square.index())) ^ flip)
}

/// A camada oculta de uma posição, vista pelas brancas e pelas pretas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accumulators {
    white: Accumulator,
    black: Accumulator,
}

impl Accumulators {
    /// Calcula do zero.
    pub fn new(net: &Network, pos: &Position) -> Accumulators {
        Accumulators {
            white: side(net, pos, Color::White),
            black: side(net, pos, Color::Black),
        }
    }

    /// Os acumuladores depois do lance `mv` (pseudo-legal em `pos`), sem recalcular do zero.
    pub fn after_move(&self, net: &Network, pos: &Position, mv: Move) -> Accumulators {
        let (from, to) = (mv.from(), mv.to());
        let moving = pos.piece_at(from).expect("lance de uma peça");
        let us = moving.color;
        // Rei que troca de bucket ou de metade do tabuleiro: o lado dele é recalculado do zero.
        if moving.kind == crate::types::PieceType::King {
            let king_to = if mv.kind() == MoveKind::Castle {
                let side = if to.file() > from.file() {
                    CastleSide::King
                } else {
                    CastleSide::Queen
                };
                castle_destinations(us, side).0
            } else {
                to
            };
            if king_key(us, from) != king_key(us, king_to) {
                let mut acc = *self;
                acc.update(net, pos, mv, us.flip());
                *acc.side_mut(us) = side(net, &pos.make_move(mv), us);
                return acc;
            }
        }
        let mut acc = *self;
        acc.update(net, pos, mv, Color::White);
        acc.update(net, pos, mv, Color::Black);
        acc
    }

    fn side_mut(&mut self, color: Color) -> &mut Accumulator {
        match color {
            Color::White => &mut self.white,
            Color::Black => &mut self.black,
        }
    }

    /// Atualiza o acumulador de `perspective` pelo lance `mv` em `pos`, com o rei desse lado onde
    /// estava (o chamador garante que o bucket e o espelho dele não mudam).
    fn update(&mut self, net: &Network, pos: &Position, mv: Move, perspective: Color) {
        let (from, to) = (mv.from(), mv.to());
        let moving = pos.piece_at(from).expect("lance de uma peça");
        let us = moving.color;
        let king = pos.king_square(perspective);
        let acc = self.side_mut(perspective);
        let mut toggle = |piece: Piece, square: Square, add: bool| {
            let column = &net.feature_weights[feature(perspective, piece, square, king)];
            if add {
                acc.add(column);
            } else {
                acc.sub(column);
            }
        };
        match mv.kind() {
            MoveKind::Castle => {
                // O lance é "rei captura torre"; as casas finais são fixas (vale no Chess960).
                let side = if to.file() > from.file() {
                    CastleSide::King
                } else {
                    CastleSide::Queen
                };
                let rook = pos.piece_at(to).expect("torre do roque");
                let (king_to, rook_to) = castle_destinations(us, side);
                toggle(moving, from, false);
                toggle(rook, to, false);
                toggle(moving, king_to, true);
                toggle(rook, rook_to, true);
            }
            MoveKind::EnPassant => {
                let victim = Square::new(to.file(), from.rank()).expect("coluna e fileira em 0..8");
                let pawn = pos.piece_at(victim).expect("peão capturado en passant");
                toggle(pawn, victim, false);
                toggle(moving, from, false);
                toggle(moving, to, true);
            }
            MoveKind::Normal | MoveKind::Promotion(_) => {
                if let Some(captured) = pos.piece_at(to) {
                    toggle(captured, to, false);
                }
                let placed = match mv.kind() {
                    MoveKind::Promotion(kind) => Piece::new(us, kind),
                    _ => moving,
                };
                toggle(moving, from, false);
                toggle(placed, to, true);
            }
        }
    }
}

/// O acumulador de um lado, calculado do zero.
fn side(net: &Network, pos: &Position, perspective: Color) -> Accumulator {
    let king = pos.king_square(perspective);
    let mut acc = net.feature_bias;
    for square in Square::all() {
        if let Some(piece) = pos.piece_at(square) {
            acc.add(&net.feature_weights[feature(perspective, piece, square, king)]);
        }
    }
    acc
}

/// Bytes de uma rede com pesos pseudoaleatórios pequenos, como o treinador salvaria.
#[cfg(test)]
pub(crate) fn random_network_bytes(seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    let mut bytes = Vec::with_capacity(NETWORK_BYTES);
    for _ in 0..NETWORK_VALUES {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let value = (state % 129) as i16 - 64;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.resize(NETWORK_BYTES, 0);
    bytes
}

#[cfg(test)]
pub(crate) fn random_network(seed: u64) -> Network {
    Network::from_bytes(&random_network_bytes(seed)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::generate_legal;
    use crate::position::STARTPOS_FEN;
    use crate::types::PieceType;

    fn sq(name: &str) -> Square {
        name.parse().unwrap()
    }

    #[test]
    fn the_embedded_network_is_a_trained_one() {
        let net = embedded();
        // Posição inicial perto de zero; uma dama a mais, vantagem enorme para quem tem a dama.
        let start = net.evaluate(&Position::startpos());
        assert!(start.abs() < 100, "{start}");
        let queen_up =
            Position::from_fen("rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        assert!(net.evaluate(&queen_up) > 500);
        let queen_down =
            Position::from_fen("rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1").unwrap();
        assert!(net.evaluate(&queen_down) < -500);
    }

    #[test]
    fn feature_index_follows_the_trainer_layout() {
        let white_pawn = Piece::new(Color::White, PieceType::Pawn);
        let black_king = Piece::new(Color::Black, PieceType::King);
        // Rei na ala da dama (c1 / c8): sem espelho, bucket 2 nos dois lados.
        // Peça do próprio lado: 0..384; do outro: 384..768. Pelas pretas, casas espelhadas.
        let base = 768 * 2;
        assert_eq!(
            feature(Color::White, white_pawn, sq("e2"), sq("c1")),
            base + 12
        );
        assert_eq!(
            feature(Color::Black, white_pawn, sq("e2"), sq("c8")),
            base + 384 + 52
        );
        assert_eq!(
            feature(Color::White, black_king, sq("e8"), sq("c1")),
            base + 384 + 5 * 64 + 60
        );
        assert_eq!(
            feature(Color::Black, black_king, sq("e8"), sq("c8")),
            base + 5 * 64 + 4
        );
    }

    #[test]
    fn king_buckets_follow_each_side_own_king_and_mirror_the_king_side() {
        let white_pawn = Piece::new(Color::White, PieceType::Pawn);
        // Rei em e1 (ala do rei): colunas espelhadas (e2 vira d2) e bucket da casa espelhada d1 = 3.
        assert_eq!(
            feature(Color::White, white_pawn, sq("e2"), sq("e1")),
            768 * 3 + 11
        );
        // Rei em a1: bucket 0; em b2: 4; em d4: 6; na quinta fileira ou além: 7.
        assert_eq!(feature(Color::White, white_pawn, sq("e2"), sq("a1")), 12);
        assert_eq!(
            feature(Color::White, white_pawn, sq("e2"), sq("b2")),
            768 * 4 + 12
        );
        assert_eq!(
            feature(Color::White, white_pawn, sq("e2"), sq("d4")),
            768 * 6 + 12
        );
        assert_eq!(
            feature(Color::White, white_pawn, sq("e2"), sq("a6")),
            768 * 7 + 12
        );
        // Pelas pretas, o rei em e8 é visto como e1: espelho e bucket 3.
        assert_eq!(
            feature(Color::Black, white_pawn, sq("e2"), sq("e8")),
            768 * 3 + ((384 + 52) ^ 7)
        );
    }

    #[test]
    fn output_bucket_follows_the_piece_count() {
        assert_eq!(output_bucket(32), 7);
        assert_eq!(output_bucket(2), 0);
        assert_eq!(output_bucket(5), 0);
        assert_eq!(output_bucket(6), 1);
        assert_eq!(output_bucket(29), 6);
    }

    #[test]
    fn a_file_of_the_wrong_size_is_rejected() {
        assert!(Network::from_bytes(&[0; 100]).is_err());
        assert!(Network::from_bytes(&vec![0; NETWORK_BYTES + 64]).is_err());
        assert!(Network::from_bytes(&vec![0; NETWORK_BYTES]).is_ok());
    }

    #[test]
    fn output_weights_must_fit_a_16_bit_product() {
        // A saída multiplica ativação (até QA) por peso em 16 bits: |peso| precisa ser ≤ 128.
        let output_start = 768 * INPUT_BUCKETS * HIDDEN + HIDDEN;
        let with_weight = |value: i16| {
            let mut bytes = vec![0u8; NETWORK_BYTES];
            let index = output_start + 7;
            bytes[2 * index..2 * index + 2].copy_from_slice(&value.to_le_bytes());
            Network::from_bytes(&bytes)
        };
        assert!(with_weight(128).is_ok());
        assert!(with_weight(-128).is_ok());
        assert_eq!(
            with_weight(129).err(),
            Some(NetworkError::OutputWeightTooLarge {
                index: 7,
                value: 129
            })
        );
        assert!(with_weight(-200).is_err());
    }

    #[test]
    fn output_follows_the_quantised_formula() {
        // Viés da camada oculta no teto (SCReLU = QA²) e pesos de saída 1 só para o lado a jogar:
        // soma = HIDDEN·QA²; /QA; ·SCALE; /(QA·QB).
        let mut bytes = vec![0u8; NETWORK_BYTES];
        let put = |bytes: &mut Vec<u8>, index: usize, value: i16| {
            bytes[2 * index..2 * index + 2].copy_from_slice(&value.to_le_bytes());
        };
        let bias_start = 768 * INPUT_BUCKETS * HIDDEN;
        let output_start = bias_start + HIDDEN;
        let bucket = output_bucket(32);
        for i in 0..HIDDEN {
            put(&mut bytes, bias_start + i, QA as i16);
            put(&mut bytes, output_start + bucket * 2 * HIDDEN + i, 1);
        }
        let net = Network::from_bytes(&bytes).unwrap();
        let expected = (HIDDEN as i32 * QA * QA / QA) * SCALE / (QA * QB);
        let pos = Position::startpos();
        assert_eq!(net.evaluate(&pos), expected);
        // Com viés de saída, soma direto na escala QA·QB.
        put(
            &mut bytes,
            output_start + OUTPUT_BUCKETS * 2 * HIDDEN + bucket,
            (QA * QB / 4) as i16,
        );
        let net = Network::from_bytes(&bytes).unwrap();
        assert_eq!(net.evaluate(&pos), expected + SCALE / 4);
    }

    #[test]
    fn evaluation_is_the_same_seen_from_either_color() {
        let net = random_network(3);
        // Cada par: a mesma posição com as cores trocadas e o tabuleiro espelhado.
        let pairs = [
            (
                STARTPOS_FEN,
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1",
            ),
            (
                "r3k2r/pp3ppp/2n1bn2/3p4/3P4/2N1BN2/PP3PPP/R3K2R w - - 0 12",
                "r3k2r/pp3ppp/2n1bn2/3p4/3P4/2N1BN2/PP3PPP/R3K2R b - - 0 12",
            ),
            (
                "8/5k2/8/3Q4/8/8/2K5/8 w - - 0 50",
                "8/2k5/8/8/3q4/8/5K2/8 b - - 0 50",
            ),
        ];
        for (a, b) in pairs {
            let a = Position::from_fen(a).unwrap();
            let b = Position::from_fen(b).unwrap();
            assert_eq!(net.evaluate(&a), net.evaluate(&b), "{}", a.to_fen());
        }
    }

    #[test]
    fn incremental_updates_match_a_full_refresh() {
        let net = random_network(11);
        let starts = [
            STARTPOS_FEN,
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9",
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
            "1n2k3/P7/8/8/8/8/8/4K3 w - - 0 1",
        ];
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for fen in starts {
            for _game in 0..10 {
                let mut pos = Position::from_fen(fen).unwrap();
                let mut acc = Accumulators::new(&net, &pos);
                for _ply in 0..100 {
                    let moves = generate_legal(&pos);
                    if moves.is_empty() {
                        break;
                    }
                    let mv = moves.as_slice()[(next() % moves.len() as u64) as usize];
                    acc = acc.after_move(&net, &pos, mv);
                    pos = pos.make_move(mv);
                    assert!(
                        acc == Accumulators::new(&net, &pos),
                        "{} após {mv:?}",
                        pos.to_fen()
                    );
                }
            }
        }
    }
}
