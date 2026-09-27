//! Avaliação estática da v1: material + tabelas de posição, interpoladas entre meio-jogo e final
//! pela fase (quantidade de peças). As tabelas são geradas por fórmulas próprias (centralização,
//! avanço de peão, segurança do rei), não copiadas de outra engine (D8). Serão substituídas por
//! ajuste com dados próprios e depois pela NNUE.

use crate::position::Position;
use crate::types::{Color, PieceType};

/// Material por tipo (peão, cavalo, bispo, torre, dama, rei), no meio-jogo e no final.
const MG_VALUE: [i32; 6] = [100, 310, 330, 500, 950, 0];
const EG_VALUE: [i32; 6] = [120, 290, 310, 530, 980, 0];
/// Peso de cada peça na fase do jogo: 24 com todas as peças, 0 só com reis e peões.
const PHASE_WEIGHT: [i32; 6] = [0, 1, 1, 2, 4, 0];
const MAX_PHASE: i32 = 24;
const BISHOP_PAIR: (i32, i32) = (25, 40);
const TEMPO: i32 = 10;

struct Tables {
    mg: [[i32; 64]; 6],
    eg: [[i32; 64]; 6],
}

/// Tabelas do ponto de vista das brancas (casas em LERF); as pretas usam a casa espelhada.
static TABLES: Tables = build_tables();

const fn build_tables() -> Tables {
    let mut mg = [[0; 64]; 6];
    let mut eg = [[0; 64]; 6];
    let mut square = 0;
    while square < 64 {
        let file = (square % 8) as i32;
        let rank = (square / 8) as i32;
        let centrality = 6 - (distance_to_center(file) + distance_to_center(rank));
        let (pawn_mg, pawn_eg) = pawn_terms(file, rank);
        mg[0][square] = pawn_mg;
        eg[0][square] = pawn_eg;
        mg[1][square] = 8 * centrality - 22;
        eg[1][square] = 6 * centrality - 18;
        mg[2][square] = 4 * centrality - 10 - if rank == 0 { 8 } else { 0 };
        eg[2][square] = 3 * centrality - 9;
        mg[3][square] = rook_mg(file, rank);
        eg[3][square] = if rank == 6 { 12 } else { 0 };
        mg[4][square] = 2 * centrality - 6;
        eg[4][square] = 4 * centrality - 12;
        mg[5][square] = king_mg(file, rank);
        eg[5][square] = 9 * centrality - 27;
        square += 1;
    }
    Tables { mg, eg }
}

/// 0 nas duas colunas (ou fileiras) centrais, 3 nas bordas.
const fn distance_to_center(x: i32) -> i32 {
    if x < 4 { 3 - x } else { x - 4 }
}

/// Peão: vale mais quanto mais avança; no meio-jogo, o centro vale um pouco mais.
const fn pawn_terms(file: i32, rank: i32) -> (i32, i32) {
    if rank == 0 || rank == 7 {
        return (0, 0);
    }
    let advance = rank - 1;
    let center = if (file == 3 || file == 4) && (rank == 3 || rank == 4) {
        10
    } else if file >= 2 && file <= 5 && rank >= 2 {
        3
    } else {
        0
    };
    let eg = 12 * advance + if rank == 6 { 20 } else { 0 };
    (5 * advance + center, eg)
}

/// Torre: sétima fileira e colunas centrais da primeira fileira.
const fn rook_mg(file: i32, rank: i32) -> i32 {
    let seventh = if rank == 6 { 20 } else { 0 };
    let central_home = if rank == 0 && (file == 3 || file == 4) {
        6
    } else {
        0
    };
    let edge = if file == 0 || file == 7 { -3 } else { 0 };
    seventh + central_home + edge
}

/// Rei no meio-jogo: abrigado na primeira fileira, de preferência nas casas de roque.
const fn king_mg(file: i32, rank: i32) -> i32 {
    let shelter = if rank == 0 {
        match file {
            1 | 2 | 6 => 18,
            0 | 7 => 10,
            3 | 5 => -5,
            _ => 0,
        }
    } else if file >= 3 && file <= 5 {
        -10
    } else {
        0
    };
    shelter - 18 * rank
}

/// Pontuação do ponto de vista do lado a jogar, em centipeões.
pub fn evaluate(pos: &Position) -> i32 {
    let mut mg = [0; 2];
    let mut eg = [0; 2];
    let mut phase = 0;
    for color in Color::ALL {
        let c = color.index();
        for kind in PieceType::ALL {
            let k = kind.index();
            let squares = pos.pieces(color, kind);
            phase += PHASE_WEIGHT[k] * squares.count() as i32;
            for square in squares.squares() {
                let index = match color {
                    Color::White => square.index(),
                    Color::Black => square.index() ^ 56,
                };
                mg[c] += MG_VALUE[k] + TABLES.mg[k][index];
                eg[c] += EG_VALUE[k] + TABLES.eg[k][index];
            }
        }
        if pos.pieces(color, PieceType::Bishop).count() >= 2 {
            mg[c] += BISHOP_PAIR.0;
            eg[c] += BISHOP_PAIR.1;
        }
    }
    let phase = phase.min(MAX_PHASE);
    let mg_score = mg[0] - mg[1];
    let eg_score = eg[0] - eg[1];
    let white_score = (mg_score * phase + eg_score * (MAX_PHASE - phase)) / MAX_PHASE;
    let score = match pos.side_to_move() {
        Color::White => white_score,
        Color::Black => -white_score,
    };
    score + TEMPO
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::STARTPOS_FEN;

    /// Espelha a posição verticalmente e troca as cores (e o lado a jogar).
    fn mirror(fen: &str) -> String {
        let fields: Vec<&str> = fen.split_whitespace().collect();
        let board: Vec<String> = fields[0]
            .split('/')
            .rev()
            .map(|rank| {
                rank.chars()
                    .map(|c| {
                        if c.is_ascii_uppercase() {
                            c.to_ascii_lowercase()
                        } else {
                            c.to_ascii_uppercase()
                        }
                    })
                    .collect()
            })
            .collect();
        let side = if fields[1] == "w" { "b" } else { "w" };
        let castling: String = fields[2]
            .chars()
            .map(|c| {
                if c == '-' {
                    c
                } else if c.is_ascii_uppercase() {
                    c.to_ascii_lowercase()
                } else {
                    c.to_ascii_uppercase()
                }
            })
            .collect();
        format!("{} {side} {castling} - 0 1", board.join("/"))
    }

    const FENS: [&str; 5] = [
        STARTPOS_FEN,
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "4k3/8/8/8/8/8/4P3/4K3 b - - 0 1",
    ];

    #[test]
    fn evaluation_is_color_symmetric() {
        for fen in FENS {
            let pos = Position::from_fen(fen).unwrap();
            let mirrored = Position::from_fen(&mirror(fen)).unwrap();
            assert_eq!(evaluate(&pos), evaluate(&mirrored), "{fen}");
        }
    }

    #[test]
    fn extra_material_is_good_for_its_owner() {
        // Brancas com uma dama a mais.
        let white_to_move = Position::from_fen("3qk3/8/8/8/8/8/8/3QK2Q w - - 0 1").unwrap();
        let black_to_move = Position::from_fen("3qk3/8/8/8/8/8/8/3QK2Q b - - 0 1").unwrap();
        assert!(evaluate(&white_to_move) > 600);
        assert!(evaluate(&black_to_move) < -600);
    }

    #[test]
    fn central_knight_beats_corner_knight() {
        let central = Position::from_fen("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1").unwrap();
        let corner = Position::from_fen("4k3/8/8/8/8/8/8/N3K3 w - - 0 1").unwrap();
        assert!(evaluate(&central) > evaluate(&corner));
    }

    #[test]
    fn advanced_passed_pawn_beats_home_pawn_in_the_endgame() {
        let advanced = Position::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let home = Position::from_fen("4k3/8/8/8/8/8/P7/4K3 w - - 0 1").unwrap();
        assert!(evaluate(&advanced) > evaluate(&home));
    }

    #[test]
    fn castled_king_is_safer_than_a_wandering_king_in_the_middlegame() {
        let castled =
            Position::from_fen("rnbq1rk1/pppp1ppp/5n2/4p3/4P3/5N2/PPPP1PPP/RNBQ1RK1 w - - 0 1")
                .unwrap();
        let wandering =
            Position::from_fen("rnbq1rk1/pppp1ppp/5n2/4p3/4P3/4KN2/PPPP1PPP/RNBQ1R2 w - - 0 1")
                .unwrap();
        assert!(evaluate(&castled) > evaluate(&wandering));
    }
}
