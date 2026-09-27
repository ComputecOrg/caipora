use std::fmt;
use std::fmt::Write as _;

use crate::attacks;
use crate::bitboard::Bitboard;
use crate::moves::{Move, MoveKind};
use crate::types::{Color, Piece, PieceType, Square};
use crate::zobrist::KEYS;

pub const STARTPOS_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// Lado do roque em relação ao rei: `King` é a direção da coluna h, `Queen` a da coluna a.
/// Em Chess960 a torre de roque pode estar em qualquer coluna daquele lado.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastleSide {
    King,
    Queen,
}

impl CastleSide {
    pub const ALL: [CastleSide; 2] = [CastleSide::King, CastleSide::Queen];

    const fn index(self) -> usize {
        self as usize
    }
}

/// Direitos de roque guardados como a casa da torre de cada lado, o que cobre xadrez normal,
/// Chess960 e DFRC com a mesma representação.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CastlingRights {
    rooks: [[Option<Square>; 2]; 2],
}

impl CastlingRights {
    pub const fn rook(&self, color: Color, side: CastleSide) -> Option<Square> {
        self.rooks[color.index()][side.index()]
    }

    pub fn set(&mut self, color: Color, side: CastleSide, rook: Square) {
        self.rooks[color.index()][side.index()] = Some(rook);
    }

    pub fn is_empty(&self) -> bool {
        self.rooks.iter().flatten().all(Option::is_none)
    }

    fn clear_color(&mut self, color: Color) {
        self.rooks[color.index()] = [None, None];
    }

    /// Remove o direito cuja torre está (ou estava) em `square`.
    fn clear_square(&mut self, square: Square) {
        for rook in self.rooks.iter_mut().flatten() {
            if *rook == Some(square) {
                *rook = None;
            }
        }
    }

    fn hash_key(&self) -> u64 {
        let mut key = 0;
        for color in Color::ALL {
            for side in CastleSide::ALL {
                if let Some(rook) = self.rook(color, side) {
                    key ^= KEYS.castling(color, side.index(), rook.file());
                }
            }
        }
        key
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FenError {
    WrongFieldCount(usize),
    BadBoard(String),
    BadSideToMove(String),
    BadCastling(String),
    BadEnPassant(String),
    BadCounter(String),
    InvalidPosition(String),
}

impl fmt::Display for FenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FenError::WrongFieldCount(n) => write!(f, "FEN com {n} campos (esperado de 4 a 6)"),
            FenError::BadBoard(s) => write!(f, "tabuleiro inválido: {s}"),
            FenError::BadSideToMove(s) => write!(f, "lado a jogar inválido: {s:?}"),
            FenError::BadCastling(s) => write!(f, "roque inválido: {s}"),
            FenError::BadEnPassant(s) => write!(f, "en passant inválido: {s}"),
            FenError::BadCounter(s) => write!(f, "contador inválido: {s:?}"),
            FenError::InvalidPosition(s) => write!(f, "posição inválida: {s}"),
        }
    }
}

impl std::error::Error for FenError {}

/// Posição completa. É `Copy` de propósito: fazer um lance copia a posição e altera a cópia
/// (copy-make), então desfazer é só voltar à cópia anterior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pieces: [Bitboard; 6],
    colors: [Bitboard; 2],
    mailbox: [Option<Piece>; 64],
    side_to_move: Color,
    castling: CastlingRights,
    /// Só existe quando alguma captura en passant é legal, para o hash e a detecção de repetição
    /// não distinguirem posições iguais.
    ep_square: Option<Square>,
    halfmove_clock: u32,
    fullmove_number: u32,
    hash: u64,
}

impl Position {
    pub fn startpos() -> Position {
        Position::from_fen(STARTPOS_FEN).expect("a FEN inicial é válida")
    }

    /// Aceita FEN padrão, X-FEN e Shredder-FEN. Os campos de meio-lance e de número do lance são
    /// opcionais (assumem 0 e 1).
    ///
    /// A casa de en passant precisa ser coerente com um avanço duplo de peão; ela é descartada
    /// quando nenhuma captura en passant é legal, para que posições iguais tenham o mesmo hash.
    pub fn from_fen(fen: &str) -> Result<Position, FenError> {
        let fields: Vec<&str> = fen.split_whitespace().collect();
        if !(4..=6).contains(&fields.len()) {
            return Err(FenError::WrongFieldCount(fields.len()));
        }
        let mut pos = Position::empty();
        pos.parse_board(fields[0])?;
        pos.side_to_move = match fields[1] {
            "w" => Color::White,
            "b" => Color::Black,
            other => return Err(FenError::BadSideToMove(other.to_string())),
        };
        pos.validate_pieces()?;
        pos.parse_castling(fields[2])?;
        pos.parse_en_passant(fields[3])?;
        if let Some(ep) = pos.ep_square
            && !pos.has_legal_ep_capture(ep)
        {
            pos.ep_square = None;
        }
        pos.halfmove_clock = parse_counter(fields.get(4), 0)?;
        pos.fullmove_number = parse_counter(fields.get(5), 1)?;
        pos.hash = pos.compute_hash();
        Ok(pos)
    }

    /// Gera X-FEN: `KQkq` quando a torre de roque é a mais externa daquele lado (o que coincide
    /// com a FEN padrão no xadrez normal) e a letra da coluna da torre nos demais casos.
    pub fn to_fen(&self) -> String {
        let mut fen = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0u8;
            for file in 0..8 {
                let square = Square::new(file, rank).expect("coluna e fileira em 0..8");
                match self.piece_at(square) {
                    Some(piece) => {
                        if empty > 0 {
                            fen.push(char::from(b'0' + empty));
                            empty = 0;
                        }
                        fen.push(piece.fen_char());
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                fen.push(char::from(b'0' + empty));
            }
            if rank > 0 {
                fen.push('/');
            }
        }
        let side = match self.side_to_move {
            Color::White => 'w',
            Color::Black => 'b',
        };
        let ep = self
            .ep_square
            .map_or_else(|| "-".to_string(), |sq| sq.to_string());
        let castling = self.castling_fen();
        write!(
            fen,
            " {side} {castling} {ep} {} {}",
            self.halfmove_clock, self.fullmove_number
        )
        .expect("escrever numa String não falha");
        fen
    }

    pub fn king_square(&self, color: Color) -> Square {
        self.pieces(color, PieceType::King)
            .squares()
            .next()
            .expect("toda posição válida tem um rei de cada cor")
    }

    fn empty() -> Position {
        Position {
            pieces: [Bitboard::EMPTY; 6],
            colors: [Bitboard::EMPTY; 2],
            mailbox: [None; 64],
            side_to_move: Color::White,
            castling: CastlingRights::default(),
            ep_square: None,
            halfmove_clock: 0,
            fullmove_number: 1,
            hash: 0,
        }
    }

    fn put_piece(&mut self, square: Square, piece: Piece) {
        let bb = Bitboard::from_square(square);
        self.pieces[piece.kind.index()] |= bb;
        self.colors[piece.color.index()] |= bb;
        self.mailbox[square.index()] = Some(piece);
        self.hash ^= KEYS.piece(piece, square);
    }

    fn parse_board(&mut self, board: &str) -> Result<(), FenError> {
        let ranks: Vec<&str> = board.split('/').collect();
        if ranks.len() != 8 {
            return Err(FenError::BadBoard(format!(
                "{} fileiras (esperado 8)",
                ranks.len()
            )));
        }
        for (row, text) in ranks.iter().enumerate() {
            let rank = 7 - row as u8;
            let too_long =
                || FenError::BadBoard(format!("fileira {} com mais de 8 casas", rank + 1));
            let mut file = 0u8;
            for c in text.chars() {
                if let Some(digit) = c.to_digit(10) {
                    if !(1..=8).contains(&digit) {
                        return Err(FenError::BadBoard(format!(
                            "dígito {c} na fileira {}",
                            rank + 1
                        )));
                    }
                    file += digit as u8;
                    if file > 8 {
                        return Err(too_long());
                    }
                } else if let Some(piece) = Piece::from_fen_char(c) {
                    let square = Square::new(file, rank).ok_or_else(too_long)?;
                    self.put_piece(square, piece);
                    file += 1;
                } else {
                    return Err(FenError::BadBoard(format!("caractere {c:?}")));
                }
            }
            if file != 8 {
                return Err(FenError::BadBoard(format!(
                    "fileira {} com {file} casas",
                    rank + 1
                )));
            }
        }
        Ok(())
    }

    fn validate_pieces(&self) -> Result<(), FenError> {
        for color in Color::ALL {
            let kings = self.pieces(color, PieceType::King).count();
            if kings != 1 {
                return Err(FenError::InvalidPosition(format!(
                    "{kings} rei(s) das {}",
                    color_name(color)
                )));
            }
        }
        const BACK_RANKS: Bitboard = Bitboard(0xFF00_0000_0000_00FF);
        if !(self.pieces[PieceType::Pawn.index()] & BACK_RANKS).is_empty() {
            return Err(FenError::InvalidPosition(
                "peão na primeira ou na última fileira".to_string(),
            ));
        }
        Ok(())
    }

    fn parse_castling(&mut self, text: &str) -> Result<(), FenError> {
        if text == "-" {
            return Ok(());
        }
        for c in text.chars() {
            let color = if c.is_ascii_uppercase() {
                Color::White
            } else {
                Color::Black
            };
            let king = self.king_square(color);
            let back_rank = back_rank(color);
            if king.rank() != back_rank {
                return Err(FenError::BadCastling(format!(
                    "{c}: rei fora da primeira fileira"
                )));
            }
            let (side, rook) = match c.to_ascii_lowercase() {
                'k' => (
                    CastleSide::King,
                    self.outermost_rook(color, CastleSide::King),
                ),
                'q' => (
                    CastleSide::Queen,
                    self.outermost_rook(color, CastleSide::Queen),
                ),
                letter @ 'a'..='h' => {
                    let file = letter as u8 - b'a';
                    let side = match file.cmp(&king.file()) {
                        std::cmp::Ordering::Greater => CastleSide::King,
                        std::cmp::Ordering::Less => CastleSide::Queen,
                        std::cmp::Ordering::Equal => {
                            return Err(FenError::BadCastling(format!(
                                "{c}: coluna do próprio rei"
                            )));
                        }
                    };
                    let square = Square::new(file, back_rank).expect("coluna e fileira em 0..8");
                    let is_rook = self.piece_at(square) == Some(Piece::new(color, PieceType::Rook));
                    (side, is_rook.then_some(square))
                }
                _ => return Err(FenError::BadCastling(format!("caractere {c:?}"))),
            };
            let rook = rook.ok_or_else(|| {
                FenError::BadCastling(format!("{c}: não há torre para esse roque"))
            })?;
            if self.castling.rook(color, side).is_some() {
                return Err(FenError::BadCastling(format!("{c}: direito repetido")));
            }
            self.castling.set(color, side, rook);
        }
        Ok(())
    }

    /// Torre da cor na primeira fileira, do lado pedido do rei, mais próxima da borda.
    fn outermost_rook(&self, color: Color, side: CastleSide) -> Option<Square> {
        let king = self.king_square(color);
        let rook = Piece::new(color, PieceType::Rook);
        let rook_on = |file: u8| {
            Square::new(file, king.rank()).filter(|&square| self.piece_at(square) == Some(rook))
        };
        match side {
            CastleSide::King => (king.file() + 1..8).rev().find_map(rook_on),
            CastleSide::Queen => (0..king.file()).find_map(rook_on),
        }
    }

    fn castling_fen(&self) -> String {
        let mut text = String::new();
        for color in Color::ALL {
            for side in CastleSide::ALL {
                let Some(rook) = self.castling.rook(color, side) else {
                    continue;
                };
                let c = if self.outermost_rook(color, side) == Some(rook) {
                    match side {
                        CastleSide::King => 'k',
                        CastleSide::Queen => 'q',
                    }
                } else {
                    char::from(b'a' + rook.file())
                };
                text.push(match color {
                    Color::White => c.to_ascii_uppercase(),
                    Color::Black => c,
                });
            }
        }
        if text.is_empty() {
            text.push('-');
        }
        text
    }

    fn parse_en_passant(&mut self, text: &str) -> Result<(), FenError> {
        if text == "-" {
            return Ok(());
        }
        let square: Square = text
            .parse()
            .map_err(|_| FenError::BadEnPassant(format!("{text:?} não é uma casa")))?;
        let us = self.side_to_move;
        // Fileira da casa de en passant, fileira onde o peão adversário parou e de onde saiu.
        let (ep_rank, landed_rank, origin_rank) = match us {
            Color::White => (5, 4, 6),
            Color::Black => (2, 3, 1),
        };
        if square.rank() != ep_rank {
            return Err(FenError::BadEnPassant(format!(
                "{text}: fileira errada para o lado a jogar"
            )));
        }
        let on_file = |rank| Square::new(square.file(), rank).expect("coluna e fileira em 0..8");
        let pushed_pawn = Piece::new(us.flip(), PieceType::Pawn);
        let double_push = self.piece_at(on_file(landed_rank)) == Some(pushed_pawn)
            && self.piece_at(square).is_none()
            && self.piece_at(on_file(origin_rank)).is_none();
        if !double_push {
            return Err(FenError::BadEnPassant(format!(
                "{text}: não houve avanço duplo de peão"
            )));
        }
        self.ep_square = Some(square);
        Ok(())
    }

    pub fn piece_at(&self, square: Square) -> Option<Piece> {
        self.mailbox[square.index()]
    }

    pub fn pieces(&self, color: Color, kind: PieceType) -> Bitboard {
        self.colors[color.index()] & self.pieces[kind.index()]
    }

    pub fn color_bb(&self, color: Color) -> Bitboard {
        self.colors[color.index()]
    }

    pub fn occupied(&self) -> Bitboard {
        self.colors[0] | self.colors[1]
    }

    pub fn side_to_move(&self) -> Color {
        self.side_to_move
    }

    pub fn castling(&self) -> CastlingRights {
        self.castling
    }

    pub fn ep_square(&self) -> Option<Square> {
        self.ep_square
    }

    pub fn halfmove_clock(&self) -> u32 {
        self.halfmove_clock
    }

    pub fn fullmove_number(&self) -> u32 {
        self.fullmove_number
    }

    /// Hash Zobrist mantido de forma incremental.
    pub fn hash(&self) -> u64 {
        self.hash
    }

    /// Hash calculado do zero; o incremental precisa sempre coincidir com ele.
    pub fn compute_hash(&self) -> u64 {
        let mut hash = 0;
        for square in Square::all() {
            if let Some(piece) = self.piece_at(square) {
                hash ^= KEYS.piece(piece, square);
            }
        }
        if self.side_to_move == Color::Black {
            hash ^= KEYS.side();
        }
        hash ^= self.castling.hash_key();
        if let Some(ep) = self.ep_square {
            hash ^= KEYS.en_passant(ep.file());
        }
        hash
    }

    /// Peças de qualquer cor que atacam `square`, com os deslizantes bloqueados por `occupied`.
    pub fn attackers_to(&self, square: Square, occupied: Bitboard) -> Bitboard {
        let diagonal =
            self.pieces[PieceType::Bishop.index()] | self.pieces[PieceType::Queen.index()];
        let straight = self.pieces[PieceType::Rook.index()] | self.pieces[PieceType::Queen.index()];
        (attacks::pawn(Color::White, square) & self.pieces(Color::Black, PieceType::Pawn))
            | (attacks::pawn(Color::Black, square) & self.pieces(Color::White, PieceType::Pawn))
            | (attacks::knight(square) & self.pieces[PieceType::Knight.index()])
            | (attacks::king(square) & self.pieces[PieceType::King.index()])
            | (attacks::bishop(square, occupied) & diagonal)
            | (attacks::rook(square, occupied) & straight)
    }

    /// `square` é atacada por alguma peça da cor `by`?
    pub fn is_attacked(&self, square: Square, by: Color) -> bool {
        !(self.attackers_to(square, self.occupied()) & self.colors[by.index()]).is_empty()
    }

    pub fn in_check(&self) -> bool {
        self.is_attacked(
            self.king_square(self.side_to_move),
            self.side_to_move.flip(),
        )
    }

    /// Nova posição depois de `mv`, que precisa ser pseudo-legal nesta posição.
    pub fn make_move(&self, mv: Move) -> Position {
        let mut next = *self;
        next.apply(mv);
        next
    }

    /// O lance pseudo-legal `mv` não deixa o próprio rei atacado?
    pub fn is_legal_after(&self, mv: Move) -> bool {
        let next = self.make_move(mv);
        !next.is_attacked(next.king_square(self.side_to_move), next.side_to_move)
    }

    fn apply(&mut self, mv: Move) {
        let us = self.side_to_move;
        let (from, to) = (mv.from(), mv.to());
        let moving = self
            .piece_at(from)
            .expect("o lance parte de uma casa com peça");
        let old_castling_key = self.castling.hash_key();
        if let Some(ep) = self.ep_square.take() {
            self.hash ^= KEYS.en_passant(ep.file());
        }
        self.halfmove_clock += 1;
        let mut double_push_target = None;
        match mv.kind() {
            MoveKind::Castle => {
                let side = if to.file() > from.file() {
                    CastleSide::King
                } else {
                    CastleSide::Queen
                };
                let (king_to, rook_to) = castle_destinations(us, side);
                // Tira as duas peças antes de recolocar: no Chess960 os destinos podem coincidir
                // com as origens.
                self.remove_piece(from);
                let rook = self.remove_piece(to);
                self.put_piece(king_to, moving);
                self.put_piece(rook_to, rook);
            }
            MoveKind::EnPassant => {
                let captured =
                    Square::new(to.file(), from.rank()).expect("coluna e fileira em 0..8");
                self.remove_piece(captured);
                self.remove_piece(from);
                self.put_piece(to, moving);
                self.halfmove_clock = 0;
            }
            MoveKind::Normal | MoveKind::Promotion(_) => {
                if self.piece_at(to).is_some() {
                    self.remove_piece(to);
                    self.halfmove_clock = 0;
                }
                self.remove_piece(from);
                let placed = match mv.kind() {
                    MoveKind::Promotion(kind) => Piece::new(us, kind),
                    _ => moving,
                };
                self.put_piece(to, placed);
                if moving.kind == PieceType::Pawn {
                    self.halfmove_clock = 0;
                    if from.rank().abs_diff(to.rank()) == 2 {
                        double_push_target =
                            Square::new(from.file(), (from.rank() + to.rank()) / 2);
                    }
                }
            }
        }
        if moving.kind == PieceType::King {
            self.castling.clear_color(us);
        }
        self.castling.clear_square(from);
        self.castling.clear_square(to);
        self.hash ^= old_castling_key ^ self.castling.hash_key();
        self.side_to_move = us.flip();
        self.hash ^= KEYS.side();
        if us == Color::Black {
            self.fullmove_number += 1;
        }
        if let Some(ep) = double_push_target
            && self.has_legal_ep_capture(ep)
        {
            self.ep_square = Some(ep);
            self.hash ^= KEYS.en_passant(ep.file());
        }
    }

    /// Existe captura en passant legal em `ep` para o lado a jogar?
    fn has_legal_ep_capture(&self, ep: Square) -> bool {
        let us = self.side_to_move;
        let capturers = attacks::pawn(us.flip(), ep) & self.pieces(us, PieceType::Pawn);
        capturers
            .squares()
            .any(|from| self.is_legal_after(Move::new(from, ep, MoveKind::EnPassant)))
    }

    fn remove_piece(&mut self, square: Square) -> Piece {
        let piece = self.mailbox[square.index()]
            .take()
            .expect("remove_piece numa casa vazia");
        let bb = Bitboard::from_square(square);
        self.pieces[piece.kind.index()] ^= bb;
        self.colors[piece.color.index()] ^= bb;
        self.hash ^= KEYS.piece(piece, square);
        piece
    }

    /// Invariante: o mailbox e os bitboards descrevem exatamente as mesmas peças.
    pub fn is_consistent(&self) -> bool {
        let mut pieces = [Bitboard::EMPTY; 6];
        let mut colors = [Bitboard::EMPTY; 2];
        for square in Square::all() {
            if let Some(piece) = self.piece_at(square) {
                pieces[piece.kind.index()] |= Bitboard::from_square(square);
                colors[piece.color.index()] |= Bitboard::from_square(square);
            }
        }
        pieces == self.pieces && colors == self.colors
    }
}

fn parse_counter(field: Option<&&str>, default: u32) -> Result<u32, FenError> {
    match field {
        None => Ok(default),
        Some(text) => text
            .parse()
            .map_err(|_| FenError::BadCounter(text.to_string())),
    }
}

/// Casas finais do rei e da torre no roque: colunas g/f no lado do rei e c/d no lado da dama,
/// iguais no xadrez normal e no Chess960.
pub fn castle_destinations(color: Color, side: CastleSide) -> (Square, Square) {
    let rank = back_rank(color);
    let (king_file, rook_file) = match side {
        CastleSide::King => (6, 5),
        CastleSide::Queen => (2, 3),
    };
    (
        Square::new(king_file, rank).expect("coluna e fileira em 0..8"),
        Square::new(rook_file, rank).expect("coluna e fileira em 0..8"),
    )
}

const fn back_rank(color: Color) -> u8 {
    match color {
        Color::White => 0,
        Color::Black => 7,
    }
}

const fn color_name(color: Color) -> &'static str {
    match color {
        Color::White => "brancas",
        Color::Black => "pretas",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(s: &str) -> Square {
        s.parse().unwrap()
    }

    const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";

    /// Posições canônicas de perft da Chessprogramming Wiki mais casos de en passant.
    const ROUND_TRIP_FENS: [&str; 8] = [
        STARTPOS_FEN,
        KIWIPETE,
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
        "rnbqkbnr/pppp1ppp/8/8/3Pp3/8/PPP1PPPP/RNBQKBNR b KQkq d3 0 2",
    ];

    #[test]
    fn startpos_is_parsed_correctly() {
        let pos = Position::startpos();
        let white_king = Piece::new(Color::White, PieceType::King);
        let black_queen = Piece::new(Color::Black, PieceType::Queen);
        assert_eq!(pos.piece_at(sq("e1")), Some(white_king));
        assert_eq!(pos.piece_at(sq("d8")), Some(black_queen));
        assert_eq!(pos.piece_at(sq("e4")), None);
        assert_eq!(pos.pieces(Color::White, PieceType::Pawn).count(), 8);
        assert_eq!(pos.occupied().count(), 32);
        assert_eq!(pos.side_to_move(), Color::White);
        let castling = pos.castling();
        assert_eq!(
            castling.rook(Color::White, CastleSide::King),
            Some(sq("h1"))
        );
        assert_eq!(
            castling.rook(Color::White, CastleSide::Queen),
            Some(sq("a1"))
        );
        assert_eq!(
            castling.rook(Color::Black, CastleSide::King),
            Some(sq("h8"))
        );
        assert_eq!(
            castling.rook(Color::Black, CastleSide::Queen),
            Some(sq("a8"))
        );
        assert_eq!(pos.ep_square(), None);
        assert_eq!((pos.halfmove_clock(), pos.fullmove_number()), (0, 1));
    }

    #[test]
    fn standard_fens_round_trip_exactly() {
        for fen in ROUND_TRIP_FENS {
            let pos = Position::from_fen(fen).unwrap_or_else(|e| panic!("{fen}: {e}"));
            assert_eq!(pos.to_fen(), fen);
            assert!(pos.is_consistent(), "{fen}");
        }
    }

    #[test]
    fn en_passant_square_is_kept() {
        let pos = Position::from_fen(ROUND_TRIP_FENS[6]).unwrap();
        assert_eq!(pos.ep_square(), Some(sq("f6")));
        let pos = Position::from_fen(ROUND_TRIP_FENS[7]).unwrap();
        assert_eq!(pos.ep_square(), Some(sq("d3")));
        assert_eq!(pos.side_to_move(), Color::Black);
    }

    #[test]
    fn move_counters_are_optional() {
        // A FEN do Kiwipete na Chessprogramming Wiki não traz os dois últimos campos.
        let pos =
            Position::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -")
                .unwrap();
        assert_eq!((pos.halfmove_clock(), pos.fullmove_number()), (0, 1));
        assert_eq!(pos.to_fen(), KIWIPETE);
    }

    #[test]
    fn shredder_fen_castling_is_understood() {
        // Posição Chess960 da suíte de perft do Ethereal (Shredder-FEN).
        let fen = "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9";
        let pos = Position::from_fen(fen).unwrap();
        let castling = pos.castling();
        assert_eq!(
            castling.rook(Color::White, CastleSide::King),
            Some(sq("h1"))
        );
        assert_eq!(
            castling.rook(Color::White, CastleSide::Queen),
            Some(sq("f1"))
        );
        assert_eq!(
            castling.rook(Color::Black, CastleSide::King),
            Some(sq("h8"))
        );
        assert_eq!(
            castling.rook(Color::Black, CastleSide::Queen),
            Some(sq("f8"))
        );
        // As duas torres são as mais externas de cada lado, então a saída usa KQkq.
        assert_eq!(
            pos.to_fen(),
            "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w KQkq - 2 9"
        );
        assert_eq!(Position::from_fen(&pos.to_fen()), Ok(pos));
    }

    #[test]
    fn x_fen_names_an_inner_castling_rook_by_its_file() {
        // Brancas: torres em a1, b1 e h1, com direito de roque grande pela torre de b1 (interna).
        // Pretas: torre de b8 é a mais externa do lado da dama, então vira 'q'.
        let fen = "1r2k1r1/8/8/8/8/8/8/RR2K2R w Bq - 0 1";
        let pos = Position::from_fen(fen).unwrap();
        let castling = pos.castling();
        assert_eq!(
            castling.rook(Color::White, CastleSide::Queen),
            Some(sq("b1"))
        );
        assert_eq!(castling.rook(Color::White, CastleSide::King), None);
        assert_eq!(
            castling.rook(Color::Black, CastleSide::Queen),
            Some(sq("b8"))
        );
        assert_eq!(castling.rook(Color::Black, CastleSide::King), None);
        assert_eq!(pos.to_fen(), fen);
    }

    #[test]
    fn no_castling_rights() {
        let pos = Position::from_fen("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1").unwrap();
        assert!(pos.castling().is_empty());
    }

    /// Joga o lance legal com a notação UCI dada (aceita as duas notações de roque).
    fn play(pos: &Position, uci: &str) -> Position {
        let mv = crate::movegen::generate_legal(pos)
            .iter()
            .find(|m| m.to_uci(false) == uci || m.to_uci(true) == uci)
            .unwrap_or_else(|| panic!("{uci} não é legal em {}", pos.to_fen()));
        pos.make_move(mv)
    }

    #[test]
    fn en_passant_square_is_dropped_when_no_pawn_can_capture() {
        let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1")
            .unwrap();
        assert_eq!(pos.ep_square(), None);
        assert_eq!(
            pos.to_fen(),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
        );
        assert_eq!(pos.hash(), play(&Position::startpos(), "e2e4").hash());
    }

    #[test]
    fn en_passant_square_is_dropped_when_the_capture_is_illegal() {
        // bxc6 e.p. tiraria b5 e c5 da quinta fileira e exporia o rei de a5 à torre de h5.
        let pos = Position::from_fen("8/8/8/KPp4r/8/8/8/7k w - c6 0 1").unwrap();
        assert_eq!(pos.ep_square(), None);
    }

    #[test]
    fn double_push_sets_en_passant_only_when_capturable() {
        let after_e4 = play(&Position::startpos(), "e2e4");
        assert_eq!(after_e4.ep_square(), None);
        let pos = Position::from_fen("rnbqkbnr/ppp1pppp/8/8/3p4/8/PPPPPPPP/RNBQKBNR w KQkq - 0 3")
            .unwrap();
        let after = play(&pos, "e2e4");
        assert_eq!(after.ep_square(), Some(sq("e3")));
        let captured = play(&after, "d4e3");
        assert_eq!(captured.piece_at(sq("e4")), None);
        assert_eq!(
            captured.piece_at(sq("e3")),
            Some(Piece::new(Color::Black, PieceType::Pawn))
        );
    }

    #[test]
    fn castling_moves_king_and_rook_and_clears_rights() {
        let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        assert_eq!(
            play(&pos, "e1g1").to_fen(),
            "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1"
        );
        assert_eq!(
            play(&pos, "e1c1").to_fen(),
            "r3k2r/8/8/8/8/8/8/2KR3R b kq - 1 1"
        );
        // Torre sai de a1 e captura a torre de a8: some um direito de cada lado.
        assert_eq!(
            play(&pos, "a1a8").to_fen(),
            "R3k2r/8/8/8/8/8/8/4K2R b Kk - 0 1"
        );
        let black = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1").unwrap();
        assert_eq!(
            play(&black, "e8g8").to_fen(),
            "r4rk1/8/8/8/8/8/8/R3K2R w KQ - 1 2"
        );
    }

    #[test]
    fn chess960_castling_where_the_king_does_not_move() {
        // Rei já em g1 e torre em h1: o roque pequeno só move a torre para f1.
        let pos = Position::from_fen("6kr/8/8/8/8/8/8/6KR w Hh - 0 1").unwrap();
        let after = play(&pos, "g1h1");
        assert_eq!(after.to_fen(), "6kr/8/8/8/8/8/8/5RK1 b k - 1 1");
    }

    #[test]
    fn promotion_replaces_the_pawn() {
        let pos = Position::from_fen("1n2k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(
            play(&pos, "a7a8q").to_fen(),
            "Qn2k3/8/8/8/8/8/8/4K3 b - - 0 1"
        );
        assert_eq!(
            play(&pos, "a7b8n").to_fen(),
            "1N2k3/8/8/8/8/8/8/4K3 b - - 0 1"
        );
    }

    /// Partidas pseudoaleatórias: o hash incremental e o invariante mailbox/bitboards precisam se
    /// manter a cada lance.
    #[test]
    fn incremental_hash_matches_full_recomputation() {
        let starts = [
            STARTPOS_FEN,
            KIWIPETE,
            "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9",
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
            "1n2k3/P7/8/8/8/8/8/4K3 w - - 0 1",
        ];
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for fen in starts {
            for _game in 0..20 {
                let mut pos = Position::from_fen(fen).unwrap();
                assert_eq!(pos.hash(), pos.compute_hash(), "{fen}");
                for _ply in 0..120 {
                    let moves = crate::movegen::generate_legal(&pos);
                    if moves.is_empty() {
                        break;
                    }
                    let mv = moves.as_slice()[(next() % moves.len() as u64) as usize];
                    pos = pos.make_move(mv);
                    assert_eq!(
                        pos.hash(),
                        pos.compute_hash(),
                        "{} após {mv:?}",
                        pos.to_fen()
                    );
                    assert!(pos.is_consistent(), "{}", pos.to_fen());
                    let reparsed = Position::from_fen(&pos.to_fen()).unwrap();
                    assert_eq!(reparsed, pos, "FEN não reconstrói a posição");
                }
            }
        }
    }

    #[test]
    fn invalid_fens_are_rejected_with_the_right_error() {
        use FenError::*;
        use std::mem::discriminant;
        let none = String::new;
        // O segundo elemento só indica a variante esperada; o texto do erro não é comparado.
        let cases = [
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w",
                WrongFieldCount(0),
            ),
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP w KQkq - 0 1",
                BadBoard(none()),
            ),
            (
                "rnbqkbnr/pppppppp/9/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                BadBoard(none()),
            ),
            (
                "rnbqkbnr/pppppppp/7/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                BadBoard(none()),
            ),
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNX w KQkq - 0 1",
                BadBoard(none()),
            ),
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR x KQkq - 0 1",
                BadSideToMove(none()),
            ),
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBKKBNR w - - 0 1",
                InvalidPosition(none()),
            ),
            (
                "rnbq1bnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQ - 0 1",
                InvalidPosition(none()),
            ),
            (
                "rnbqkbnP/pppppppp/8/8/8/8/PPPPPPP1/RNBQKBNR w - - 0 1",
                InvalidPosition(none()),
            ),
            (
                "rnbqkbn1/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                BadCastling(none()),
            ),
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq e9 0 1",
                BadEnPassant(none()),
            ),
            (
                "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e3 0 1",
                BadEnPassant(none()),
            ),
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - x 1",
                BadCounter(none()),
            ),
        ];
        for (fen, expected) in cases {
            let err = Position::from_fen(fen).expect_err(fen);
            assert_eq!(
                discriminant(&err),
                discriminant(&expected),
                "{fen}: erro inesperado {err:?}"
            );
        }
    }
}
