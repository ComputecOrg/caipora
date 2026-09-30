//! Leitura de vários binpacks intercalados bloco a bloco (um bloco de cada arquivo por vez), com um
//! buffer embaralhado: mistura meses e fontes sem cópia intercalada em disco (em 30/09/2026 uma cópia
//! assim encheu o C: do dono). Lê em volta, sem fim; o treino decide quando parar.

use std::{fs::File, io::BufReader};

use bullet::game::formats::bulletformat::ChessBoard;
use bullet_trainer::reader::DataReader;
use sfbinpack::{
    ChunkReader, TrainingDataEntry,
    chess::{color::Color, piecetype::PieceType},
    read_chunk_into,
};

#[derive(Clone)]
pub struct InterleavedBinpacks {
    paths: Vec<String>,
    /// Posições por lote embaralhado.
    buffer: usize,
    filter: fn(&TrainingDataEntry) -> bool,
}

impl InterleavedBinpacks {
    pub fn new(paths: &[&str], buffer: usize, filter: fn(&TrainingDataEntry) -> bool) -> Self {
        Self {
            paths: paths.iter().map(|p| p.to_string()).collect(),
            buffer,
            filter,
        }
    }
}

impl DataReader<ChessBoard> for InterleavedBinpacks {
    fn read_chunks<F: FnMut(&[ChessBoard]) -> bool>(&self, _skip: usize, mut f: F) {
        let mut rng = 0x9E37_79B9_7F4A_7C15_u64;
        let mut buffer = Vec::with_capacity(self.buffer);
        let mut chunk = Vec::new();
        loop {
            let mut readers: Vec<Option<BufReader<File>>> = self
                .paths
                .iter()
                .map(|p| Some(BufReader::new(File::open(p).expect("binpack não abre"))))
                .collect();
            while readers.iter().any(Option::is_some) {
                for slot in readers.iter_mut() {
                    let Some(reader) = slot else { continue };
                    if !read_chunk_into(reader, &mut chunk).unwrap_or(false) {
                        *slot = None;
                        continue;
                    }
                    let mut entries = ChunkReader::default();
                    while entries.has_next(&chunk) {
                        let entry = entries.next(&chunk);
                        if !(self.filter)(&entry) {
                            continue;
                        }
                        buffer.push(to_board(&entry));
                        if buffer.len() == self.buffer {
                            shuffle(&mut buffer, &mut rng);
                            if f(&buffer) {
                                return;
                            }
                            buffer.clear();
                        }
                    }
                }
            }
        }
    }
}

// Uma thread basta: medido em 30/09/2026, o treino com buckets fica com a GPU em 99% a ~320 mil
// posições/s; decodificar em paralelo não mudou a velocidade.

/// Posição do binpack no formato do bullet (pontuação e resultado das brancas; o bullet vira para o
/// lado a jogar).
fn to_board(entry: &TrainingDataEntry) -> ChessBoard {
    let piece = |pt| {
        entry.pos.pieces_bb_color(Color::Black, pt).bits()
            | entry.pos.pieces_bb_color(Color::White, pt).bits()
    };
    let bbs = [
        entry.pos.pieces_bb(Color::White).bits(),
        entry.pos.pieces_bb(Color::Black).bits(),
        piece(PieceType::Pawn),
        piece(PieceType::Knight),
        piece(PieceType::Bishop),
        piece(PieceType::Rook),
        piece(PieceType::Queen),
        piece(PieceType::King),
    ];
    let stm = usize::from(entry.pos.side_to_move().ordinal());
    let (mut score, mut result) = (entry.score, f32::from(1 + entry.result) / 2.0);
    if stm > 0 {
        score = -score;
        result = 1.0 - result;
    }
    ChessBoard::from_raw(bbs, stm, score, result).expect("binpack com posição inválida")
}

fn shuffle(data: &mut [ChessBoard], state: &mut u64) {
    for i in (1..data.len()).rev() {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        data.swap(i, (*state % (i as u64 + 1)) as usize);
    }
}
