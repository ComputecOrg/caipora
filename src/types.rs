use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub const ALL: [Color; 2] = [Color::White, Color::Black];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn flip(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PieceType {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl PieceType {
    pub const ALL: [PieceType; 6] = [
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
        PieceType::King,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Piece {
    pub color: Color,
    pub kind: PieceType,
}

impl Piece {
    pub const fn new(color: Color, kind: PieceType) -> Piece {
        Piece { color, kind }
    }

    /// Letra da FEN: maiúscula para as brancas, minúscula para as pretas.
    pub fn from_fen_char(c: char) -> Option<Piece> {
        let kind = match c.to_ascii_lowercase() {
            'p' => PieceType::Pawn,
            'n' => PieceType::Knight,
            'b' => PieceType::Bishop,
            'r' => PieceType::Rook,
            'q' => PieceType::Queen,
            'k' => PieceType::King,
            _ => return None,
        };
        let color = if c.is_ascii_uppercase() {
            Color::White
        } else {
            Color::Black
        };
        Some(Piece::new(color, kind))
    }

    pub fn fen_char(self) -> char {
        let c = match self.kind {
            PieceType::Pawn => 'p',
            PieceType::Knight => 'n',
            PieceType::Bishop => 'b',
            PieceType::Rook => 'r',
            PieceType::Queen => 'q',
            PieceType::King => 'k',
        };
        match self.color {
            Color::White => c.to_ascii_uppercase(),
            Color::Black => c,
        }
    }
}

/// Casa do tabuleiro em LERF: a1 = 0, b1 = 1, ..., h8 = 63.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Square(u8);

impl Square {
    pub const fn from_index(index: u8) -> Option<Square> {
        if index < 64 {
            Some(Square(index))
        } else {
            None
        }
    }

    /// `file` e `rank` em 0..8.
    pub const fn new(file: u8, rank: u8) -> Option<Square> {
        if file < 8 && rank < 8 {
            Some(Square(rank * 8 + file))
        } else {
            None
        }
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub const fn file(self) -> u8 {
        self.0 % 8
    }

    pub const fn rank(self) -> u8 {
        self.0 / 8
    }

    pub fn all() -> impl Iterator<Item = Square> {
        (0..64).map(Square)
    }

    /// Casa deslocada `df` colunas e `dr` fileiras; `None` se sair do tabuleiro.
    pub const fn offset(self, df: i8, dr: i8) -> Option<Square> {
        let file = self.file() as i8 + df;
        let rank = self.rank() as i8 + dr;
        if file >= 0 && file < 8 && rank >= 0 && rank < 8 {
            Some(Square((rank * 8 + file) as u8))
        } else {
            None
        }
    }

    /// Usa só os 6 bits baixos de `bits`, então sempre é uma casa válida.
    pub const fn from_low_bits(bits: u16) -> Square {
        Square((bits & 63) as u8)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseSquareError(pub String);

impl fmt::Display for ParseSquareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "casa inválida: {:?}", self.0)
    }
}

impl std::error::Error for ParseSquareError {}

impl FromStr for Square {
    type Err = ParseSquareError;

    fn from_str(s: &str) -> Result<Square, ParseSquareError> {
        let err = || ParseSquareError(s.to_string());
        let &[file, rank] = s.as_bytes() else {
            return Err(err());
        };
        if !(b'a'..=b'h').contains(&file) || !(b'1'..=b'8').contains(&rank) {
            return Err(err());
        }
        Square::new(file - b'a', rank - b'1').ok_or_else(err)
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let file = char::from(b'a' + self.file());
        let rank = char::from(b'1' + self.rank());
        write!(f, "{file}{rank}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(s: &str) -> Square {
        s.parse().unwrap()
    }

    #[test]
    fn square_indices_follow_lerf() {
        assert_eq!(sq("a1").index(), 0);
        assert_eq!(sq("h1").index(), 7);
        assert_eq!(sq("e4").index(), 28);
        assert_eq!(sq("a8").index(), 56);
        assert_eq!(sq("h8").index(), 63);
    }

    #[test]
    fn square_file_and_rank() {
        let e4 = sq("e4");
        assert_eq!((e4.file(), e4.rank()), (4, 3));
        assert_eq!(Square::new(4, 3), Some(e4));
        assert_eq!(Square::new(8, 0), None);
        assert_eq!(Square::new(0, 8), None);
        assert_eq!(Square::from_index(63), Some(sq("h8")));
        assert_eq!(Square::from_index(64), None);
    }

    #[test]
    fn square_display_round_trips_for_all_64() {
        for square in Square::all() {
            assert_eq!(square.to_string().parse::<Square>(), Ok(square));
        }
        assert_eq!(Square::all().count(), 64);
    }

    #[test]
    fn square_rejects_invalid_text() {
        for bad in ["", "e", "i1", "a9", "a0", "e44", "E4", "4e"] {
            assert!(
                bad.parse::<Square>().is_err(),
                "{bad:?} deveria ser inválida"
            );
        }
    }

    #[test]
    fn square_offset_stays_on_the_board() {
        assert_eq!(sq("e4").offset(1, 2), Some(sq("f6")));
        assert_eq!(sq("e4").offset(-4, -3), Some(sq("a1")));
        assert_eq!(sq("a1").offset(-1, 0), None);
        assert_eq!(sq("h8").offset(0, 1), None);
        assert_eq!(sq("h4").offset(1, 0), None);
    }

    #[test]
    fn color_flip() {
        assert_eq!(Color::White.flip(), Color::Black);
        assert_eq!(Color::Black.flip(), Color::White);
    }

    #[test]
    fn piece_fen_chars_round_trip() {
        for (c, color, kind) in [
            ('P', Color::White, PieceType::Pawn),
            ('n', Color::Black, PieceType::Knight),
            ('B', Color::White, PieceType::Bishop),
            ('r', Color::Black, PieceType::Rook),
            ('Q', Color::White, PieceType::Queen),
            ('k', Color::Black, PieceType::King),
        ] {
            let piece = Piece::new(color, kind);
            assert_eq!(Piece::from_fen_char(c), Some(piece));
            assert_eq!(piece.fen_char(), c);
        }
        assert_eq!(Piece::from_fen_char('x'), None);
        assert_eq!(Piece::from_fen_char('1'), None);
    }
}
