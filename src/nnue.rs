//! Avaliação por rede neural (NNUE), no formato quantizado que o treinador `bullet` salva.
//!
//! Arquitetura (768 → HIDDEN)×2 → 1. As entradas são peça × casa vistas de cada lado. A camada
//! oculta é mantida de forma incremental: um acumulador por perspectiva, atualizado lance a
//! lance. A ativação é SCReLU (x limitado a [0, QA], ao quadrado).
//!
//! O arquivo guarda, em i16 little-endian e nesta ordem:
//! - os pesos da camada oculta, 768 × HIDDEN, agrupados por entrada;
//! - o viés da camada oculta (HIDDEN);
//! - os pesos de saída (2·HIDDEN): primeiro o lado a jogar, depois o outro;
//! - o viés de saída.
//!
//! O arquivo é completado com zeros até um múltiplo de 64 bytes.

use crate::moves::{Move, MoveKind};
use crate::position::{CastleSide, Position, castle_destinations};
use crate::types::{Color, Piece, Square};

/// Neurônios da camada oculta.
pub const HIDDEN: usize = 256;
/// Quantização da camada oculta: 1,0 vira `QA`.
const QA: i32 = 255;
/// Quantização dos pesos de saída.
const QB: i32 = 64;
/// Saída da rede (em unidades de vitória) para centipeões.
const SCALE: i32 = 400;
/// Número de valores i16 no arquivo, sem o preenchimento.
const NETWORK_VALUES: usize = 768 * HIDDEN + HIDDEN + 2 * HIDDEN + 1;
/// Tamanho do arquivo: os valores, completados até múltiplo de 64 bytes.
pub const NETWORK_BYTES: usize = (2 * NETWORK_VALUES).div_ceil(64) * 64;

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
    /// Uma coluna por entrada.
    feature_weights: Vec<Accumulator>,
    feature_bias: Accumulator,
    output_weights: [i16; 2 * HIDDEN],
    output_bias: i16,
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
        let feature_weights = (0..768).map(|_| column()).collect();
        let feature_bias = column();
        let mut output_weights = [0; 2 * HIDDEN];
        for (slot, value) in output_weights.iter_mut().zip(&mut values) {
            *slot = value;
        }
        let output_bias = values.next().expect("o tamanho já foi conferido");
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
        self.output(&Accumulators::new(self, pos), pos.side_to_move())
    }

    /// Saída da rede a partir dos acumuladores já calculados.
    pub fn output(&self, acc: &Accumulators, side_to_move: Color) -> i32 {
        let (us, them) = match side_to_move {
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
        let sum =
            half(us, &self.output_weights[..HIDDEN]) + half(them, &self.output_weights[HIDDEN..]);
        (sum / QA + i32::from(self.output_bias)) * SCALE / (QA * QB)
    }
}

/// Índice da entrada "peça em casa" vista pelo lado `perspective`: 0..384 para as peças dele,
/// 384..768 para as do outro; pelas pretas, o tabuleiro é espelhado na vertical.
pub fn feature(perspective: Color, piece: Piece, square: Square) -> usize {
    let side = if piece.color == perspective { 0 } else { 384 };
    let square = match perspective {
        Color::White => square.index(),
        Color::Black => square.index() ^ 56,
    };
    side + 64 * piece.kind.index() + square
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
        let mut acc = Accumulators {
            white: net.feature_bias,
            black: net.feature_bias,
        };
        for square in Square::all() {
            if let Some(piece) = pos.piece_at(square) {
                acc.toggle(net, piece, square, true);
            }
        }
        acc
    }

    /// Os acumuladores depois do lance `mv` (pseudo-legal em `pos`), sem recalcular do zero.
    pub fn after_move(&self, net: &Network, pos: &Position, mv: Move) -> Accumulators {
        let mut acc = *self;
        let (from, to) = (mv.from(), mv.to());
        let moving = pos.piece_at(from).expect("lance de uma peça");
        let us = moving.color;
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
                acc.toggle(net, moving, from, false);
                acc.toggle(net, rook, to, false);
                acc.toggle(net, moving, king_to, true);
                acc.toggle(net, rook, rook_to, true);
            }
            MoveKind::EnPassant => {
                let victim = Square::new(to.file(), from.rank()).expect("coluna e fileira em 0..8");
                let pawn = pos.piece_at(victim).expect("peão capturado en passant");
                acc.toggle(net, pawn, victim, false);
                acc.toggle(net, moving, from, false);
                acc.toggle(net, moving, to, true);
            }
            MoveKind::Normal | MoveKind::Promotion(_) => {
                if let Some(captured) = pos.piece_at(to) {
                    acc.toggle(net, captured, to, false);
                }
                let placed = match mv.kind() {
                    MoveKind::Promotion(kind) => Piece::new(us, kind),
                    _ => moving,
                };
                acc.toggle(net, moving, from, false);
                acc.toggle(net, placed, to, true);
            }
        }
        acc
    }

    fn toggle(&mut self, net: &Network, piece: Piece, square: Square, add: bool) {
        let white = &net.feature_weights[feature(Color::White, piece, square)];
        let black = &net.feature_weights[feature(Color::Black, piece, square)];
        if add {
            self.white.add(white);
            self.black.add(black);
        } else {
            self.white.sub(white);
            self.black.sub(black);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::generate_legal;
    use crate::position::STARTPOS_FEN;
    use crate::types::PieceType;

    /// Rede com pesos pseudoaleatórios pequenos, montada em bytes como o treinador salvaria.
    fn random_network(seed: u64) -> Network {
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
        Network::from_bytes(&bytes).unwrap()
    }

    fn sq(name: &str) -> Square {
        name.parse().unwrap()
    }

    #[test]
    fn feature_index_follows_the_trainer_layout() {
        let white_pawn = Piece::new(Color::White, PieceType::Pawn);
        let black_king = Piece::new(Color::Black, PieceType::King);
        // Peça do próprio lado: 0..384; do outro: 384..768. Pelas pretas, casas espelhadas.
        assert_eq!(feature(Color::White, white_pawn, sq("e2")), 12);
        assert_eq!(feature(Color::Black, white_pawn, sq("e2")), 384 + 52);
        assert_eq!(
            feature(Color::White, black_king, sq("e8")),
            384 + 5 * 64 + 60
        );
        assert_eq!(feature(Color::Black, black_king, sq("e8")), 5 * 64 + 4);
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
        let output_start = 768 * HIDDEN + HIDDEN;
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
        let bias_start = 768 * HIDDEN;
        let output_start = bias_start + HIDDEN;
        for i in 0..HIDDEN {
            put(&mut bytes, bias_start + i, QA as i16);
            put(&mut bytes, output_start + i, 1);
        }
        let net = Network::from_bytes(&bytes).unwrap();
        let expected = (HIDDEN as i32 * QA * QA / QA) * SCALE / (QA * QB);
        let pos = Position::startpos();
        assert_eq!(net.evaluate(&pos), expected);
        // Com viés de saída, soma direto na escala QA·QB.
        put(&mut bytes, output_start + 2 * HIDDEN, (QA * QB / 4) as i16);
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
