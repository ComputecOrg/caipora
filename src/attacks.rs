//! Tabelas de ataque. Cavalo, rei e peão usam tabelas diretas; bispo e torre usam "fancy" magic
//! bitboards com números mágicos gerados aqui mesmo, a partir de uma semente fixa. Não usamos
//! PEXT: é microcodificado (lento) no Zen 1/2, a CPU da máquina de desenvolvimento.

use std::sync::LazyLock;

use crate::bitboard::Bitboard;
use crate::types::{Color, Square};

const ROOK_DIRECTIONS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRECTIONS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

const KNIGHT_OFFSETS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];
const KING_OFFSETS: [(i8, i8); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

pub fn knight(square: Square) -> Bitboard {
    TABLES.knight[square.index()]
}

pub fn king(square: Square) -> Bitboard {
    TABLES.king[square.index()]
}

/// Casas atacadas por um peão da cor `color` em `square`.
pub fn pawn(color: Color, square: Square) -> Bitboard {
    TABLES.pawn[color.index()][square.index()]
}

pub fn bishop(square: Square, occupied: Bitboard) -> Bitboard {
    let tables = &*TABLES;
    tables.sliders[tables.bishop_magics[square.index()].index(occupied)]
}

pub fn rook(square: Square, occupied: Bitboard) -> Bitboard {
    let tables = &*TABLES;
    tables.sliders[tables.rook_magics[square.index()].index(occupied)]
}

pub fn queen(square: Square, occupied: Bitboard) -> Bitboard {
    bishop(square, occupied) | rook(square, occupied)
}

/// Casas estritamente entre `a` e `b` quando estão na mesma linha, coluna ou diagonal; vazio
/// caso contrário.
pub fn between(a: Square, b: Square) -> Bitboard {
    TABLES.between[a.index() * 64 + b.index()]
}

struct Magic {
    mask: u64,
    magic: u64,
    shift: u32,
    offset: usize,
}

impl Magic {
    fn index(&self, occupied: Bitboard) -> usize {
        self.offset + ((occupied.0 & self.mask).wrapping_mul(self.magic) >> self.shift) as usize
    }
}

struct Tables {
    knight: [Bitboard; 64],
    king: [Bitboard; 64],
    pawn: [[Bitboard; 64]; 2],
    bishop_magics: Vec<Magic>,
    rook_magics: Vec<Magic>,
    /// Ataques de bispo e de torre para todas as ocupações relevantes, indexados pelos magics.
    sliders: Vec<Bitboard>,
    between: Vec<Bitboard>,
}

static TABLES: LazyLock<Tables> = LazyLock::new(Tables::build);

impl Tables {
    fn build() -> Tables {
        let pawn_offsets = [[(-1, 1), (1, 1)], [(-1, -1), (1, -1)]];
        let mut rng = XorShift64(0x5EED_CA1F_0DA7_A5E7);
        let mut sliders = Vec::new();
        let bishop_magics = Square::all()
            .map(|sq| find_magic(sq, &BISHOP_DIRECTIONS, &mut sliders, &mut rng))
            .collect();
        let rook_magics = Square::all()
            .map(|sq| find_magic(sq, &ROOK_DIRECTIONS, &mut sliders, &mut rng))
            .collect();
        Tables {
            knight: leaper_table(&KNIGHT_OFFSETS),
            king: leaper_table(&KING_OFFSETS),
            pawn: [
                leaper_table(&pawn_offsets[0]),
                leaper_table(&pawn_offsets[1]),
            ],
            bishop_magics,
            rook_magics,
            sliders,
            between: between_table(),
        }
    }
}

fn leaper_table(offsets: &[(i8, i8)]) -> [Bitboard; 64] {
    let mut table = [Bitboard::EMPTY; 64];
    for square in Square::all() {
        for &(df, dr) in offsets {
            if let Some(target) = square.offset(df, dr) {
                table[square.index()] |= Bitboard::from_square(target);
            }
        }
    }
    table
}

fn between_table() -> Vec<Bitboard> {
    let mut table = vec![Bitboard::EMPTY; 64 * 64];
    for from in Square::all() {
        for &(df, dr) in ROOK_DIRECTIONS.iter().chain(&BISHOP_DIRECTIONS) {
            let mut ray = Bitboard::EMPTY;
            let mut current = from;
            while let Some(next) = current.offset(df, dr) {
                table[from.index() * 64 + next.index()] = ray;
                ray |= Bitboard::from_square(next);
                current = next;
            }
        }
    }
    table
}

/// Casas cuja ocupação altera os ataques: o raio sem a última casa antes da borda.
fn relevant_mask(square: Square, directions: &[(i8, i8)]) -> u64 {
    let mut mask = 0;
    for &(df, dr) in directions {
        let mut current = square;
        while let Some(next) = current.offset(df, dr) {
            if next.offset(df, dr).is_none() {
                break;
            }
            mask |= Bitboard::from_square(next).0;
            current = next;
        }
    }
    mask
}

/// Procura por tentativa um número mágico sem colisões destrutivas para `square` e grava os
/// ataques correspondentes no fim de `table`.
fn find_magic(
    square: Square,
    directions: &[(i8, i8)],
    table: &mut Vec<Bitboard>,
    rng: &mut XorShift64,
) -> Magic {
    let mask = relevant_mask(square, directions);
    let bits = mask.count_ones();
    let shift = 64 - bits;
    let size = 1usize << bits;
    let mut occupancies = Vec::with_capacity(size);
    let mut subset = 0u64;
    loop {
        let attacks = slider_attacks_slow(square, Bitboard(subset), directions);
        occupancies.push((subset, attacks));
        subset = subset.wrapping_sub(mask) & mask;
        if subset == 0 {
            break;
        }
    }
    // Cada entrada guarda em qual tentativa foi escrita, para não limpar a tabela a cada tentativa.
    let mut slots = vec![(0u32, Bitboard::EMPTY); size];
    let mut attempt = 0u32;
    loop {
        let magic = rng.sparse();
        if (mask.wrapping_mul(magic) >> 56).count_ones() < 6 {
            continue;
        }
        attempt += 1;
        let collision_free = occupancies.iter().all(|&(occupied, attacks)| {
            let slot = &mut slots[(occupied.wrapping_mul(magic) >> shift) as usize];
            if slot.0 != attempt {
                *slot = (attempt, attacks);
                true
            } else {
                slot.1 == attacks
            }
        });
        if collision_free {
            let offset = table.len();
            table.extend(slots.iter().map(|&(written, attacks)| {
                if written == attempt {
                    attacks
                } else {
                    Bitboard::EMPTY
                }
            }));
            return Magic {
                mask,
                magic,
                shift,
                offset,
            };
        }
    }
}

struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Candidato com poucos bits ligados, que costuma virar magic mais rápido.
    fn sparse(&mut self) -> u64 {
        self.next() & self.next() & self.next()
    }
}

/// Ataques de peça deslizante calculados raio a raio: lento, serve de referência para gerar as
/// tabelas mágicas e para os testes.
fn slider_attacks_slow(square: Square, occupied: Bitboard, directions: &[(i8, i8)]) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;
    for &(df, dr) in directions {
        let mut current = square;
        while let Some(next) = current.offset(df, dr) {
            attacks |= Bitboard::from_square(next);
            if occupied.contains(next) {
                break;
            }
            current = next;
        }
    }
    attacks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(s: &str) -> Square {
        s.parse().unwrap()
    }

    fn bb(squares: &[&str]) -> Bitboard {
        squares
            .iter()
            .fold(Bitboard::EMPTY, |acc, s| acc | Bitboard::from_square(sq(s)))
    }

    #[test]
    fn leaper_attacks() {
        assert_eq!(knight(sq("a1")), bb(&["b3", "c2"]));
        assert_eq!(knight(sq("e4")).count(), 8);
        assert_eq!(king(sq("h8")), bb(&["g8", "g7", "h7"]));
        assert_eq!(king(sq("e1")).count(), 5);
        assert_eq!(pawn(Color::White, sq("e4")), bb(&["d5", "f5"]));
        assert_eq!(pawn(Color::Black, sq("e4")), bb(&["d3", "f3"]));
        assert_eq!(pawn(Color::White, sq("a2")), bb(&["b3"]));
        assert_eq!(pawn(Color::Black, sq("h7")), bb(&["g6"]));
    }

    #[test]
    fn slider_attacks_stop_at_blockers() {
        let occupied = bb(&["a4", "c1"]);
        assert_eq!(
            rook(sq("a1"), occupied),
            bb(&["a2", "a3", "a4", "b1", "c1"])
        );
        let occupied = bb(&["c3", "g7"]);
        assert_eq!(
            bishop(sq("e5"), occupied),
            bb(&["d6", "c7", "b8", "f6", "g7", "d4", "c3", "f4", "g3", "h2"])
        );
        assert_eq!(queen(sq("d1"), Bitboard(u64::MAX)).count(), 5);
    }

    /// Compara as tabelas mágicas com a referência lenta em ocupações pseudoaleatórias.
    #[test]
    fn magic_lookup_matches_slow_reference() {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for square in Square::all() {
            for _ in 0..200 {
                let occupied = Bitboard(next() & next());
                assert_eq!(
                    rook(square, occupied),
                    slider_attacks_slow(square, occupied, &ROOK_DIRECTIONS),
                    "torre em {square}"
                );
                assert_eq!(
                    bishop(square, occupied),
                    slider_attacks_slow(square, occupied, &BISHOP_DIRECTIONS),
                    "bispo em {square}"
                );
            }
        }
    }

    #[test]
    fn squares_between() {
        assert_eq!(
            between(sq("a1"), sq("h8")),
            bb(&["b2", "c3", "d4", "e5", "f6", "g7"])
        );
        assert_eq!(
            between(sq("e8"), sq("e1")),
            bb(&["e2", "e3", "e4", "e5", "e6", "e7"])
        );
        assert_eq!(between(sq("e1"), sq("f1")), Bitboard::EMPTY);
        assert_eq!(between(sq("a1"), sq("b3")), Bitboard::EMPTY);
        assert_eq!(between(sq("c4"), sq("c4")), Bitboard::EMPTY);
    }
}
