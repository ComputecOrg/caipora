use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Not};

use crate::types::Square;

/// Conjunto de casas: o bit `i` corresponde à casa de índice LERF `i`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Bitboard(pub u64);

impl Bitboard {
    pub const EMPTY: Bitboard = Bitboard(0);

    pub const fn from_square(square: Square) -> Bitboard {
        Bitboard(1 << square.index())
    }

    pub const fn contains(self, square: Square) -> bool {
        self.0 & (1 << square.index()) != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }

    pub fn squares(self) -> Squares {
        Squares(self.0)
    }
}

/// Itera as casas do conjunto em ordem crescente de índice.
pub struct Squares(u64);

impl Iterator for Squares {
    type Item = Square;

    fn next(&mut self) -> Option<Square> {
        if self.0 == 0 {
            return None;
        }
        let index = self.0.trailing_zeros() as u8;
        self.0 &= self.0 - 1;
        Square::from_index(index)
    }
}

impl BitOr for Bitboard {
    type Output = Bitboard;
    fn bitor(self, rhs: Bitboard) -> Bitboard {
        Bitboard(self.0 | rhs.0)
    }
}

impl BitAnd for Bitboard {
    type Output = Bitboard;
    fn bitand(self, rhs: Bitboard) -> Bitboard {
        Bitboard(self.0 & rhs.0)
    }
}

impl BitXor for Bitboard {
    type Output = Bitboard;
    fn bitxor(self, rhs: Bitboard) -> Bitboard {
        Bitboard(self.0 ^ rhs.0)
    }
}

impl Not for Bitboard {
    type Output = Bitboard;
    fn not(self) -> Bitboard {
        Bitboard(!self.0)
    }
}

impl BitOrAssign for Bitboard {
    fn bitor_assign(&mut self, rhs: Bitboard) {
        self.0 |= rhs.0;
    }
}

impl BitAndAssign for Bitboard {
    fn bitand_assign(&mut self, rhs: Bitboard) {
        self.0 &= rhs.0;
    }
}

impl BitXorAssign for Bitboard {
    fn bitxor_assign(&mut self, rhs: Bitboard) {
        self.0 ^= rhs.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(s: &str) -> Square {
        s.parse().unwrap()
    }

    #[test]
    fn single_square_sets_its_lerf_bit() {
        assert_eq!(Bitboard::from_square(sq("a1")), Bitboard(1));
        assert_eq!(Bitboard::from_square(sq("h8")), Bitboard(1 << 63));
        assert!(Bitboard::from_square(sq("e4")).contains(sq("e4")));
        assert!(!Bitboard::from_square(sq("e4")).contains(sq("e5")));
    }

    #[test]
    fn count_and_emptiness() {
        assert!(Bitboard::EMPTY.is_empty());
        assert_eq!(Bitboard::EMPTY.count(), 0);
        let bb = Bitboard::from_square(sq("a1")) | Bitboard::from_square(sq("h8"));
        assert!(!bb.is_empty());
        assert_eq!(bb.count(), 2);
        assert_eq!(Bitboard(u64::MAX).count(), 64);
    }

    #[test]
    fn squares_are_iterated_in_ascending_order() {
        let bb = Bitboard::from_square(sq("h8"))
            | Bitboard::from_square(sq("a1"))
            | Bitboard::from_square(sq("e4"));
        let got: Vec<Square> = bb.squares().collect();
        assert_eq!(got, vec![sq("a1"), sq("e4"), sq("h8")]);
        assert_eq!(Bitboard(u64::MAX).squares().count(), 64);
        assert_eq!(Bitboard::EMPTY.squares().next(), None);
    }

    #[test]
    fn set_operations() {
        let a = Bitboard(0b1100);
        let b = Bitboard(0b1010);
        assert_eq!(a | b, Bitboard(0b1110));
        assert_eq!(a & b, Bitboard(0b1000));
        assert_eq!(a ^ b, Bitboard(0b0110));
        assert_eq!(!Bitboard::EMPTY, Bitboard(u64::MAX));
        let mut c = a;
        c ^= b;
        c |= Bitboard(1);
        c &= Bitboard(0b0111);
        assert_eq!(c, Bitboard(0b0111));
    }
}
