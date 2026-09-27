use std::fmt;
use std::num::NonZeroU16;

use crate::types::{PieceType, Square};

/// Tipo do lance. No roque, o destino guardado é a casa da torre ("rei captura a própria torre"),
/// o que cobre Chess960 sem casos especiais.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveKind {
    Normal,
    Castle,
    EnPassant,
    Promotion(PieceType),
}

/// Lance em 16 bits: origem (bits 0–5), destino (6–11) e tipo (12–15).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Move(NonZeroU16);

const KIND_NORMAL: u16 = 0;
const KIND_CASTLE: u16 = 1;
const KIND_EN_PASSANT: u16 = 2;
/// Promoções usam 4 + (cavalo 0, bispo 1, torre 2, dama 3).
const KIND_PROMOTION: u16 = 4;

impl Move {
    pub fn new(from: Square, to: Square, kind: MoveKind) -> Move {
        debug_assert_ne!(from, to, "origem e destino iguais");
        let code = match kind {
            MoveKind::Normal => KIND_NORMAL,
            MoveKind::Castle => KIND_CASTLE,
            MoveKind::EnPassant => KIND_EN_PASSANT,
            MoveKind::Promotion(piece) => {
                KIND_PROMOTION
                    + match piece {
                        PieceType::Knight => 0,
                        PieceType::Bishop => 1,
                        PieceType::Rook => 2,
                        PieceType::Queen => 3,
                        PieceType::Pawn | PieceType::King => panic!("promoção para {piece:?}"),
                    }
            }
        };
        let raw = from.index() as u16 | (to.index() as u16) << 6 | code << 12;
        Move(NonZeroU16::new(raw).expect("origem e destino diferentes nunca dão zero"))
    }

    pub fn from(self) -> Square {
        Square::from_low_bits(self.0.get())
    }

    pub fn to(self) -> Square {
        Square::from_low_bits(self.0.get() >> 6)
    }

    pub fn kind(self) -> MoveKind {
        match self.0.get() >> 12 {
            KIND_NORMAL => MoveKind::Normal,
            KIND_CASTLE => MoveKind::Castle,
            KIND_EN_PASSANT => MoveKind::EnPassant,
            code => MoveKind::Promotion(match code - KIND_PROMOTION {
                0 => PieceType::Knight,
                1 => PieceType::Bishop,
                2 => PieceType::Rook,
                _ => PieceType::Queen,
            }),
        }
    }

    /// Notação UCI. No roque, com `chess960` o destino é a torre (e1h1); sem, é a casa final do
    /// rei (e1g1).
    pub fn to_uci(self, chess960: bool) -> String {
        let from = self.from();
        let mut to = self.to();
        if self.kind() == MoveKind::Castle && !chess960 {
            let king_file = if to.file() > from.file() { 6 } else { 2 };
            to = Square::new(king_file, from.rank()).expect("coluna e fileira em 0..8");
        }
        let mut text = format!("{from}{to}");
        if let MoveKind::Promotion(piece) = self.kind() {
            text.push(match piece {
                PieceType::Knight => 'n',
                PieceType::Bishop => 'b',
                PieceType::Rook => 'r',
                _ => 'q',
            });
        }
        text
    }
}

/// Valor de preenchimento das posições não usadas da `MoveList` (a1b1, nunca lido).
const FILLER: Move = Move(match NonZeroU16::new(1 << 6) {
    Some(raw) => raw,
    None => panic!("1 << 6 não é zero"),
});

impl fmt::Debug for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_uci(true))
    }
}

pub const MAX_MOVES: usize = 256;

/// Lista de lances de capacidade fixa, sem alocação.
#[derive(Clone)]
pub struct MoveList {
    moves: [Move; MAX_MOVES],
    len: usize,
}

impl MoveList {
    pub fn new() -> MoveList {
        MoveList {
            moves: [FILLER; MAX_MOVES],
            len: 0,
        }
    }

    pub fn push(&mut self, mv: Move) {
        self.moves[self.len] = mv;
        self.len += 1;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..self.len]
    }

    pub fn as_mut_slice(&mut self) -> &mut [Move] {
        &mut self.moves[..self.len]
    }

    pub fn iter(&self) -> std::iter::Copied<std::slice::Iter<'_, Move>> {
        self.as_slice().iter().copied()
    }

    pub fn contains(&self, mv: Move) -> bool {
        self.as_slice().contains(&mv)
    }
}

impl Default for MoveList {
    fn default() -> MoveList {
        MoveList::new()
    }
}

impl<'a> IntoIterator for &'a MoveList {
    type Item = Move;
    type IntoIter = std::iter::Copied<std::slice::Iter<'a, Move>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(s: &str) -> Square {
        s.parse().unwrap()
    }

    #[test]
    fn fields_round_trip() {
        let kinds = [
            MoveKind::Normal,
            MoveKind::Castle,
            MoveKind::EnPassant,
            MoveKind::Promotion(PieceType::Knight),
            MoveKind::Promotion(PieceType::Bishop),
            MoveKind::Promotion(PieceType::Rook),
            MoveKind::Promotion(PieceType::Queen),
        ];
        for from in Square::all() {
            for to in Square::all().filter(|&to| to != from) {
                for kind in kinds {
                    let mv = Move::new(from, to, kind);
                    assert_eq!((mv.from(), mv.to(), mv.kind()), (from, to, kind));
                }
            }
        }
        assert_eq!(std::mem::size_of::<Option<Move>>(), 2);
    }

    #[test]
    fn uci_notation() {
        assert_eq!(
            Move::new(sq("e2"), sq("e4"), MoveKind::Normal).to_uci(false),
            "e2e4"
        );
        let promo = Move::new(sq("e7"), sq("e8"), MoveKind::Promotion(PieceType::Queen));
        assert_eq!(promo.to_uci(false), "e7e8q");
        let underpromo = Move::new(sq("b2"), sq("a1"), MoveKind::Promotion(PieceType::Knight));
        assert_eq!(underpromo.to_uci(true), "b2a1n");
        let ep = Move::new(sq("e5"), sq("d6"), MoveKind::EnPassant);
        assert_eq!(ep.to_uci(false), "e5d6");
    }

    #[test]
    fn castling_notation_depends_on_chess960_mode() {
        let short = Move::new(sq("e1"), sq("h1"), MoveKind::Castle);
        assert_eq!(short.to_uci(false), "e1g1");
        assert_eq!(short.to_uci(true), "e1h1");
        let long = Move::new(sq("e8"), sq("a8"), MoveKind::Castle);
        assert_eq!(long.to_uci(false), "e8c8");
        assert_eq!(long.to_uci(true), "e8a8");
        // Chess960: rei em b1, torre do lado da dama em a1; o rei termina em c1.
        let frc = Move::new(sq("b1"), sq("a1"), MoveKind::Castle);
        assert_eq!(frc.to_uci(false), "b1c1");
    }

    #[test]
    fn move_list_push_and_iterate() {
        let mut list = MoveList::new();
        assert!(list.is_empty());
        let a = Move::new(sq("e2"), sq("e4"), MoveKind::Normal);
        let b = Move::new(sq("g1"), sq("f3"), MoveKind::Normal);
        list.push(a);
        list.push(b);
        assert_eq!(list.len(), 2);
        assert_eq!(list.iter().collect::<Vec<_>>(), vec![a, b]);
        assert!(list.contains(b));
        assert!(!list.contains(Move::new(sq("d2"), sq("d4"), MoveKind::Normal)));
    }
}
