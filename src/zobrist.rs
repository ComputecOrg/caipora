//! Chaves Zobrist de 64 bits geradas em tempo de compilação com uma semente fixa, para que o hash
//! (e portanto o `bench`) seja idêntico em toda máquina.

use crate::types::{Color, Piece, Square};

pub struct Keys {
    pieces: [[[u64; 64]; 6]; 2],
    side: u64,
    /// Por cor, lado do roque e coluna da torre (a coluna importa no Chess960).
    castling: [[[u64; 8]; 2]; 2],
    en_passant: [u64; 8],
}

pub static KEYS: Keys = Keys::generate();

impl Keys {
    const fn generate() -> Keys {
        // Semente arbitrária ("CAIPORA1" em ASCII); mudá-la muda todos os hashes e o bench.
        let mut rng = SplitMix64(0x4341_4950_4F52_4131);
        let mut pieces = [[[0; 64]; 6]; 2];
        let mut color = 0;
        while color < 2 {
            let mut kind = 0;
            while kind < 6 {
                let mut square = 0;
                while square < 64 {
                    pieces[color][kind][square] = rng.next();
                    square += 1;
                }
                kind += 1;
            }
            color += 1;
        }
        let side = rng.next();
        let mut castling = [[[0; 8]; 2]; 2];
        let mut color = 0;
        while color < 2 {
            let mut side_index = 0;
            while side_index < 2 {
                let mut file = 0;
                while file < 8 {
                    castling[color][side_index][file] = rng.next();
                    file += 1;
                }
                side_index += 1;
            }
            color += 1;
        }
        let mut en_passant = [0; 8];
        let mut file = 0;
        while file < 8 {
            en_passant[file] = rng.next();
            file += 1;
        }
        Keys {
            pieces,
            side,
            castling,
            en_passant,
        }
    }

    pub fn piece(&self, piece: Piece, square: Square) -> u64 {
        self.pieces[piece.color.index()][piece.kind.index()][square.index()]
    }

    pub fn side(&self) -> u64 {
        self.side
    }

    /// `side_index`: 0 para o lado do rei, 1 para o lado da dama.
    pub fn castling(&self, color: Color, side_index: usize, rook_file: u8) -> u64 {
        self.castling[color.index()][side_index][rook_file as usize]
    }

    pub fn en_passant(&self, file: u8) -> u64 {
        self.en_passant[file as usize]
    }
}

/// Gerador SplitMix64 (Steele, Lea e Flood), avaliável em tempo de compilação.
struct SplitMix64(u64);

impl SplitMix64 {
    const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PieceType;

    #[test]
    fn keys_are_distinct_and_nonzero() {
        let mut all = Vec::new();
        for color in Color::ALL {
            for kind in PieceType::ALL {
                for square in Square::all() {
                    all.push(KEYS.piece(Piece::new(color, kind), square));
                }
            }
            for side in 0..2 {
                for file in 0..8 {
                    all.push(KEYS.castling(color, side, file));
                }
            }
        }
        for file in 0..8 {
            all.push(KEYS.en_passant(file));
        }
        all.push(KEYS.side());
        assert_eq!(all.len(), 12 * 64 + 32 + 8 + 1);
        assert!(all.iter().all(|&k| k != 0));
        let mut sorted = all.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len(), "chaves repetidas");
    }
}
