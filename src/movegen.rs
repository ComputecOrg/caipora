//! Geração de lances: pseudo-legais por tipo de peça, filtrados por legalidade (o próprio rei não
//! pode ficar atacado depois do lance). O roque é gerado já com as condições de caminho livre e de
//! casas não atacadas, valendo para xadrez normal e Chess960.

use crate::attacks;
use crate::bitboard::Bitboard;
use crate::moves::{Move, MoveKind, MoveList};
use crate::position::{CastleSide, Position, castle_destinations};
use crate::types::{Color, PieceType, Square};

/// Todos os lances pseudo-legais: podem deixar o próprio rei em xeque. O roque só é gerado quando
/// o rei não está em xeque, o caminho está livre e o rei não atravessa casa atacada; a casa final
/// do rei é conferida pela legalidade depois do lance.
pub fn generate_pseudo_legal(pos: &Position, list: &mut MoveList) {
    let us = pos.side_to_move();
    let own = pos.color_bb(us);
    let enemy = pos.color_bb(us.flip());
    let occupied = own | enemy;
    generate_pawn_moves(pos, list, occupied, enemy);
    for kind in [
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
        PieceType::King,
    ] {
        for from in pos.pieces(us, kind).squares() {
            let targets = match kind {
                PieceType::Knight => attacks::knight(from),
                PieceType::Bishop => attacks::bishop(from, occupied),
                PieceType::Rook => attacks::rook(from, occupied),
                PieceType::Queen => attacks::queen(from, occupied),
                _ => attacks::king(from),
            } & !own;
            for to in targets.squares() {
                list.push(Move::new(from, to, MoveKind::Normal));
            }
        }
    }
    generate_castling(pos, list);
}

fn generate_pawn_moves(pos: &Position, list: &mut MoveList, occupied: Bitboard, enemy: Bitboard) {
    let us = pos.side_to_move();
    let (forward, start_rank) = match us {
        Color::White => (1, 1),
        Color::Black => (-1, 6),
    };
    for from in pos.pieces(us, PieceType::Pawn).squares() {
        if let Some(one) = from.offset(0, forward)
            && !occupied.contains(one)
        {
            push_pawn_move(list, from, one);
            if from.rank() == start_rank
                && let Some(two) = one.offset(0, forward)
                && !occupied.contains(two)
            {
                list.push(Move::new(from, two, MoveKind::Normal));
            }
        }
        let captures = attacks::pawn(us, from);
        for to in (captures & enemy).squares() {
            push_pawn_move(list, from, to);
        }
        if let Some(ep) = pos.ep_square()
            && captures.contains(ep)
        {
            list.push(Move::new(from, ep, MoveKind::EnPassant));
        }
    }
}

/// Avanço ou captura de peão, desdobrado nas quatro promoções quando chega à última fileira.
fn push_pawn_move(list: &mut MoveList, from: Square, to: Square) {
    if to.rank() == 0 || to.rank() == 7 {
        for piece in [
            PieceType::Queen,
            PieceType::Knight,
            PieceType::Rook,
            PieceType::Bishop,
        ] {
            list.push(Move::new(from, to, MoveKind::Promotion(piece)));
        }
    } else {
        list.push(Move::new(from, to, MoveKind::Normal));
    }
}

fn generate_castling(pos: &Position, list: &mut MoveList) {
    let us = pos.side_to_move();
    let rights = pos.castling();
    if rights.rook(us, CastleSide::King).is_none() && rights.rook(us, CastleSide::Queen).is_none() {
        return;
    }
    if pos.in_check() {
        return;
    }
    let king_from = pos.king_square(us);
    for side in CastleSide::ALL {
        let Some(rook_from) = rights.rook(us, side) else {
            continue;
        };
        let (king_to, rook_to) = castle_destinations(us, side);
        let others =
            pos.occupied() ^ Bitboard::from_square(king_from) ^ Bitboard::from_square(rook_from);
        let king_path = attacks::between(king_from, king_to) | Bitboard::from_square(king_to);
        let rook_path = attacks::between(rook_from, rook_to) | Bitboard::from_square(rook_to);
        if !((king_path | rook_path) & others).is_empty() {
            continue;
        }
        // Casas atravessadas pelo rei (sem a origem, coberta pelo xeque, e sem o destino,
        // conferido depois do lance, quando a torre já está na casa final).
        let crossed = attacks::between(king_from, king_to);
        if crossed.squares().any(|sq| pos.is_attacked(sq, us.flip())) {
            continue;
        }
        list.push(Move::new(king_from, rook_from, MoveKind::Castle));
    }
}

pub fn generate_legal(pos: &Position) -> MoveList {
    let mut pseudo = MoveList::new();
    generate_pseudo_legal(pos, &mut pseudo);
    let mut legal = MoveList::new();
    for mv in &pseudo {
        if pos.is_legal_after(mv) {
            legal.push(mv);
        }
    }
    legal
}

/// Número de folhas da árvore de lances legais até `depth`.
pub fn perft(pos: &Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = generate_legal(pos);
    if depth == 1 {
        return moves.len() as u64;
    }
    moves
        .iter()
        .map(|mv| perft(&pos.make_move(mv), depth - 1))
        .sum()
}

/// Perft por lance da raiz, para comparar com outra engine e achar a divergência.
pub fn divide(pos: &Position, depth: u32) -> Vec<(Move, u64)> {
    generate_legal(pos)
        .iter()
        .map(|mv| {
            let nodes = if depth <= 1 {
                1
            } else {
                perft(&pos.make_move(mv), depth - 1)
            };
            (mv, nodes)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_has_twenty_legal_moves() {
        let moves = generate_legal(&Position::startpos());
        let mut uci: Vec<String> = moves.iter().map(|m| m.to_uci(false)).collect();
        uci.sort();
        let mut expected = vec![
            "a2a3", "a2a4", "b1a3", "b1c3", "b2b3", "b2b4", "c2c3", "c2c4", "d2d3", "d2d4", "e2e3",
            "e2e4", "f2f3", "f2f4", "g1f3", "g1h3", "g2g3", "g2g4", "h2h3", "h2h4",
        ];
        expected.sort();
        assert_eq!(uci, expected);
    }

    #[test]
    fn checkmated_and_stalemated_sides_have_no_moves() {
        // Mate do louco.
        let mate =
            Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
                .unwrap();
        assert!(mate.in_check());
        assert!(generate_legal(&mate).is_empty());
        let stalemate = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert!(!stalemate.in_check());
        assert!(generate_legal(&stalemate).is_empty());
    }

    #[test]
    fn castling_is_blocked_by_attacked_squares() {
        // Bispo preto em a6 ataca f1: o roque pequeno das brancas é ilegal, o grande não.
        let pos = Position::from_fen("r3k2r/8/b7/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        let uci: Vec<String> = generate_legal(&pos)
            .iter()
            .map(|m| m.to_uci(false))
            .collect();
        assert!(!uci.contains(&"e1g1".to_string()));
        assert!(uci.contains(&"e1c1".to_string()));
    }

    #[test]
    fn divide_sums_to_perft() {
        let pos = Position::startpos();
        let total: u64 = divide(&pos, 3).iter().map(|&(_, n)| n).sum();
        assert_eq!(total, perft(&pos, 3));
    }
}
