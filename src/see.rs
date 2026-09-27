//! Static exchange evaluation: saldo material de uma sequência de capturas numa única casa,
//! com cada lado usando sempre o atacante mais barato e podendo parar quando não compensa.
//! Considera peças escondidas atrás de outras (raio X); ignora cravadas e xeques.

use crate::bitboard::Bitboard;
use crate::moves::{Move, MoveKind};
use crate::position::Position;
use crate::types::{PieceType, Square};

/// Valores de troca por tipo de peça (peão, cavalo, bispo, torre, dama, rei).
pub const SEE_VALUE: [i32; 6] = [100, 320, 330, 500, 900, 20_000];

/// Saldo do lance `mv` para quem o joga, em centipeões. Roque vale 0.
pub fn see(pos: &Position, mv: Move) -> i32 {
    let (from, to) = (mv.from(), mv.to());
    let Some(moving) = pos.piece_at(from) else {
        return 0;
    };
    let mut occupied = pos.occupied() ^ Bitboard::from_square(from);
    let captured = match mv.kind() {
        MoveKind::Castle => return 0,
        MoveKind::EnPassant => {
            let victim = Square::new(to.file(), from.rank()).expect("coluna e fileira em 0..8");
            occupied ^= Bitboard::from_square(victim);
            SEE_VALUE[PieceType::Pawn.index()]
        }
        _ => pos
            .piece_at(to)
            .map_or(0, |piece| SEE_VALUE[piece.kind.index()]),
    };
    // gain[d]: saldo, para quem captura na etapa d, supondo que a troca pare ali.
    let mut gain = [0i32; 32];
    gain[0] = captured;
    // Valor da peça que está na casa e pode ser capturada a seguir.
    let mut on_square = match mv.kind() {
        MoveKind::Promotion(kind) => SEE_VALUE[kind.index()],
        _ => SEE_VALUE[moving.kind.index()],
    };
    let mut side = moving.color.flip();
    let mut depth = 0;
    loop {
        let attackers = pos.attackers_to(to, occupied) & occupied;
        let ours = attackers & pos.color_bb(side);
        let Some((kind, square)) = least_valuable_attacker(pos, ours) else {
            break;
        };
        // O rei só captura se o outro lado não puder recapturar.
        if kind == PieceType::King && !(attackers & pos.color_bb(side.flip())).is_empty() {
            break;
        }
        depth += 1;
        gain[depth] = on_square - gain[depth - 1];
        on_square = SEE_VALUE[kind.index()];
        occupied ^= Bitboard::from_square(square);
        side = side.flip();
        if depth + 1 == gain.len() {
            break;
        }
    }
    while depth > 0 {
        gain[depth - 1] = -(-gain[depth - 1]).max(gain[depth]);
        depth -= 1;
    }
    gain[0]
}

fn least_valuable_attacker(pos: &Position, attackers: Bitboard) -> Option<(PieceType, Square)> {
    PieceType::ALL.into_iter().find_map(|kind| {
        let of_kind = attackers
            & (pos.pieces(crate::types::Color::White, kind)
                | pos.pieces(crate::types::Color::Black, kind));
        of_kind.squares().next().map(|square| (kind, square))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::generate_legal;

    fn see_of(fen: &str, uci: &str) -> i32 {
        let pos = Position::from_fen(fen).unwrap();
        let mv = generate_legal(&pos)
            .iter()
            .find(|m| m.to_uci(false) == uci)
            .unwrap_or_else(|| panic!("{uci} não é legal em {fen}"));
        see(&pos, mv)
    }

    #[test]
    fn undefended_pawn_is_a_clean_win() {
        assert_eq!(see_of("4k3/8/8/4p3/8/8/8/4RK2 w - - 0 1", "e1e5"), 100);
    }

    #[test]
    fn queen_takes_pawn_defended_by_pawn() {
        assert_eq!(see_of("4k3/8/3p4/4p3/8/8/8/4QK2 w - - 0 1", "e1e5"), -800);
    }

    #[test]
    fn pawn_takes_knight_defended_by_pawn() {
        assert_eq!(see_of("4k3/8/5p2/4n3/3P4/8/8/4K3 w - - 0 1", "d4e5"), 220);
    }

    #[test]
    fn doubled_rooks_see_through_each_other() {
        // Rxe5, e a torre preta não recaptura porque a segunda torre branca está atrás.
        assert_eq!(see_of("4k3/4r3/8/4p3/8/8/4R3/4RK2 w - - 0 1", "e2e5"), 100);
    }

    #[test]
    fn the_king_recaptures_only_an_undefended_piece() {
        assert_eq!(see_of("8/8/3k4/4p3/8/8/8/4RK2 w - - 0 1", "e1e5"), -400);
        assert_eq!(see_of("8/8/3k4/4p3/8/2B5/8/4RK2 w - - 0 1", "e1e5"), 100);
    }

    #[test]
    fn en_passant_capture() {
        assert_eq!(see_of("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6"), 100);
    }

    #[test]
    fn quiet_move_to_an_attacked_square_loses_the_piece() {
        // Cavalo vai para uma casa atacada por peão.
        assert_eq!(see_of("4k3/8/3p4/8/8/5N2/8/4K3 w - - 0 1", "f3e5"), -320);
        assert_eq!(see_of("4k3/8/3p4/8/8/5N2/8/4K3 w - - 0 1", "f3g5"), 0);
    }
}
