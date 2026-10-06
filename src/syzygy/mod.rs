//! Tablebases Syzygy de finais: leitura própria do formato, a partir da especificação em
//! `docs/syzygy-spec.md` (nenhum código de terceiros).
//!
//! As tabelas são indexadas pelo nome ao abrir a pasta e carregadas na memória só quando uma
//! posição daquele material é sondada pela primeira vez.

pub mod encode;
pub mod table;

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use crate::movegen::generate_legal;
use crate::moves::MoveKind;
use crate::position::Position;
use crate::types::{Color, PieceType};
use table::{Kind, Table};

/// Resultado teórico do ponto de vista de quem joga. "Amaldiçoada" e "abençoada" são vitória e
/// derrota que a regra dos 50 lances transforma em empate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Wdl {
    Loss,
    BlessedLoss,
    Draw,
    CursedWin,
    Win,
}

impl Wdl {
    fn from_value(v: i32) -> Wdl {
        match v {
            ..=-2 => Wdl::Loss,
            -1 => Wdl::BlessedLoss,
            0 => Wdl::Draw,
            1 => Wdl::CursedWin,
            _ => Wdl::Win,
        }
    }
}

struct Entry {
    wdl_path: std::path::PathBuf,
    wdl: OnceLock<Option<Loaded>>,
}

struct Loaded {
    table: Table,
    /// Material do "branco da tabela" (pelas peças do descritor), ex. "KQ".
    white: String,
}

pub struct Tablebases {
    entries: HashMap<String, Entry>,
    max_pieces: usize,
}

const PIECE_LETTERS: [(PieceType, char); 6] = [
    (PieceType::King, 'K'),
    (PieceType::Queen, 'Q'),
    (PieceType::Rook, 'R'),
    (PieceType::Bishop, 'B'),
    (PieceType::Knight, 'N'),
    (PieceType::Pawn, 'P'),
];

fn strength(c: char) -> u8 {
    match c {
        'K' => 6,
        'Q' => 5,
        'R' => 4,
        'B' => 3,
        'N' => 2,
        _ => 1,
    }
}

/// O lado que vai à esquerda no nome do arquivo: mais peças; empate, a primeira peça mais forte.
fn left_first(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return a.len() > b.len();
    }
    for (x, y) in a.chars().zip(b.chars()) {
        if x != y {
            return strength(x) > strength(y);
        }
    }
    true
}

fn material(pos: &Position, color: Color) -> String {
    let mut s = String::new();
    for (kind, letter) in PIECE_LETTERS {
        for _ in 0..pos.pieces(color, kind).count() {
            s.push(letter);
        }
    }
    s
}

/// Código de peça do formato: 1 peão … 6 rei, + 8 para o "preto da tabela".
fn piece_code(kind: PieceType, table_black: bool) -> u8 {
    let base = match kind {
        PieceType::Pawn => 1,
        PieceType::Knight => 2,
        PieceType::Bishop => 3,
        PieceType::Rook => 4,
        PieceType::Queen => 5,
        PieceType::King => 6,
    };
    base | if table_black { 8 } else { 0 }
}

fn code_letter(code: u8) -> char {
    match code & 7 {
        1 => 'P',
        2 => 'N',
        3 => 'B',
        4 => 'R',
        5 => 'Q',
        _ => 'K',
    }
}

impl Tablebases {
    /// Abre as pastas de `paths` (separadas por `;` ou, fora do Windows, também por `:`).
    /// Pastas inexistentes são ignoradas.
    pub fn open(paths: &str) -> Tablebases {
        let separators: &[char] = if cfg!(windows) { &[';'] } else { &[';', ':'] };
        let mut entries = HashMap::new();
        let mut max_pieces = 0;
        for dir in paths.split(separators).filter(|p| !p.is_empty()) {
            let Ok(listing) = std::fs::read_dir(Path::new(dir)) else {
                continue;
            };
            for file in listing.flatten() {
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("rtbw") {
                    continue;
                }
                let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Some((pieces, _, _)) = table::parse_name(name) else {
                    continue;
                };
                max_pieces = max_pieces.max(pieces);
                entries.entry(name.to_string()).or_insert(Entry {
                    wdl_path: path.clone(),
                    wdl: OnceLock::new(),
                });
            }
        }
        Tablebases {
            entries,
            max_pieces,
        }
    }

    /// Maior número de peças (reis incluídos) coberto; 0 sem tabelas.
    pub fn max_pieces(&self) -> usize {
        self.max_pieces
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    fn load(&self, name: &str) -> Option<&Loaded> {
        let entry = self.entries.get(name)?;
        entry
            .wdl
            .get_or_init(|| {
                let data = std::fs::read(&entry.wdl_path).ok()?;
                let table = Table::parse(data, Kind::Wdl, name)?;
                let first = &table.subtable(0, 0).pieces;
                let mut white: Vec<char> = first
                    .iter()
                    .filter(|&&c| c & 8 == 0)
                    .map(|&c| code_letter(c))
                    .collect();
                white.sort_by_key(|&c| std::cmp::Reverse(strength(c)));
                Some(Loaded {
                    table,
                    white: white.into_iter().collect(),
                })
            })
            .as_ref()
    }

    /// WDL da posição, resolvendo capturas e en passant (seção 6.1). `None` fora das tabelas:
    /// com direito de roque, peças demais ou tabela ausente.
    pub fn probe_wdl(&self, pos: &Position) -> Option<Wdl> {
        if self.max_pieces == 0
            || !pos.castling().is_empty()
            || pos.occupied().count() as usize > self.max_pieces
            || Color::ALL
                .iter()
                .any(|&c| pos.pieces(c, PieceType::King).count() != 1)
        {
            return None;
        }
        self.wdl_value(pos).map(Wdl::from_value)
    }

    fn wdl_value(&self, pos: &Position) -> Option<i32> {
        if pos.occupied().count() == 2 {
            return Some(0);
        }
        let moves = generate_legal(pos);
        if moves.is_empty() {
            return Some(if pos.in_check() { -2 } else { 0 });
        }
        let mut best = -3;
        let mut captures = 0;
        let mut only_captures = true;
        for mv in &moves {
            let capture = mv.kind() == MoveKind::EnPassant
                || (mv.kind() != MoveKind::Castle && pos.piece_at(mv.to()).is_some());
            if !capture {
                only_captures = false;
                continue;
            }
            captures += 1;
            let v = -self.wdl_value(&pos.make_move(mv))?;
            best = best.max(v);
            if best == 2 {
                return Some(2);
            }
        }
        if captures > 0 && only_captures {
            return Some(best);
        }
        let stored = self.table_wdl(pos)?;
        Some(if captures > 0 {
            best.max(stored)
        } else {
            stored
        })
    }

    /// Valor guardado na tabela (seções 3–5), sem olhar capturas.
    fn table_wdl(&self, pos: &Position) -> Option<i32> {
        let white = material(pos, Color::White);
        let black = material(pos, Color::Black);
        let name = if left_first(&white, &black) {
            format!("{white}v{black}")
        } else {
            format!("{black}v{white}")
        };
        let loaded = self.load(&name)?;
        let table = &loaded.table;
        let black_to_move = pos.side_to_move() == Color::Black;
        let flip = if table.symmetric {
            black_to_move
        } else {
            white != loaded.white
        };
        let side = if table.sides == 2 {
            usize::from(black_to_move != flip)
        } else {
            0
        };

        // Casas de cada código de peça, já no referencial da tabela.
        let mut by_code: [Vec<usize>; 16] = Default::default();
        for color in Color::ALL {
            let table_black = (color == Color::Black) != flip;
            for (kind, _) in PIECE_LETTERS {
                for sq in pos.pieces(color, kind).squares() {
                    let mut s = sq.index();
                    if flip && table.has_pawns {
                        s = encode::mirror_vertical(s);
                    }
                    by_code[usize::from(piece_code(kind, table_black))].push(s);
                }
            }
        }

        let (sub, leader) = if table.has_pawns {
            let lead_code = usize::from(table.subtable(0, side).pieces[0]);
            let t = &*encode::TABLES;
            let (i, &best) = by_code[lead_code]
                .iter()
                .enumerate()
                .max_by_key(|&(_, &s)| t.pawn_twist(s))?;
            by_code[lead_code].swap_remove(i);
            (
                table.subtable(encode::edge_distance(best), side),
                Some(best),
            )
        } else {
            (table.subtable(0, side), None)
        };
        let mut squares = Vec::with_capacity(sub.pieces.len());
        for (i, &code) in sub.pieces.iter().enumerate() {
            if i == 0
                && let Some(l) = leader
            {
                squares.push(l);
                continue;
            }
            squares.push(by_code[usize::from(code)].pop()?);
        }
        let idx = encode::index(&mut squares, &sub.groups, table.has_pawns)?;
        if idx >= sub.size {
            return None;
        }
        Some(i32::from(table.value(sub, idx)?) - 2)
    }
}
