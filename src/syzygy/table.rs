//! Um arquivo Syzygy em memória: cabeçalho, sub-tabelas e descompressão dos valores
//! (docs/syzygy-spec.md, seções 1, 2 e 4).

use super::encode::{self, Group};

const WDL_MAGIC: [u8; 4] = [0x71, 0xE8, 0x23, 0x5D];
const DTZ_MAGIC: [u8; 4] = [0xD7, 0x66, 0x0C, 0xA5];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Wdl,
    Dtz,
}

/// Dados comprimidos de uma sub-tabela (seção 4).
#[derive(Debug)]
pub struct Pairs {
    pub flags: u8,
    single: Option<u16>,
    block_size: u32,
    idx_bits: u32,
    real_blocks: u32,
    size_entries: u32,
    min_len: u32,
    lowest: Vec<u16>,
    base: Vec<u64>,
    symbols: Vec<[u8; 3]>,
    symlen: Vec<u32>,
    index_offset: usize,
    sizes_offset: usize,
    data_offset: usize,
}

#[derive(Debug)]
pub struct SubTable {
    pub pieces: Vec<u8>,
    pub groups: Vec<Group>,
    pub size: u64,
    pub pairs: Pairs,
}

/// Mapa DTZ de uma coluna: deslocamento de cada classe (vitória, derrota, amaldiçoada,
/// abençoada) e se as entradas são de 16 bits.
#[derive(Debug, Clone, Copy)]
pub struct DtzMap {
    pub offsets: [usize; 4],
    pub wide: bool,
}

#[derive(Debug)]
pub struct Table {
    data: Vec<u8>,
    pub kind: Kind,
    pub has_pawns: bool,
    pub symmetric: bool,
    /// Sub-tabelas por coluna (1 sem peões, 4 com) e lado (2 em WDL não simétrica, senão 1).
    pub sub: Vec<SubTable>,
    pub sides: usize,
    pub maps: Vec<Option<DtzMap>>,
}

fn u16_le(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(at)?, *d.get(at + 1)?]))
}

fn u32_le(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// Leitura big-endian que trata bytes além do fim como zero (seção 4.5, bordas).
fn be(d: &[u8], at: usize, bytes: usize) -> u64 {
    (0..bytes).fold(0u64, |acc, i| {
        acc << 8 | u64::from(d.get(at + i).copied().unwrap_or(0))
    })
}

/// Material de uma tabela pelo nome (ex. "KRPvKR"): contagem de peças de cada lado e se os
/// dois lados têm peões.
pub fn parse_name(name: &str) -> Option<(usize, bool, bool)> {
    let (white, black) = name.split_once('v')?;
    let valid = |s: &str| s.starts_with('K') && s.chars().all(|c| "KQRBNP".contains(c));
    if !valid(white) || !valid(black) {
        return None;
    }
    let pawns = |s: &str| s.contains('P');
    Some((
        white.len() + black.len(),
        pawns(white) || pawns(black),
        pawns(white) && pawns(black),
    ))
}

impl Table {
    pub fn parse(data: Vec<u8>, kind: Kind, name: &str) -> Option<Table> {
        let magic = if kind == Kind::Wdl {
            WDL_MAGIC
        } else {
            DTZ_MAGIC
        };
        if data.get(0..4)? != magic || data.len() % 64 != 16 {
            return None;
        }
        let (pieces_count, has_pawns, both_pawns) = parse_name(name)?;
        let flags = data[4];
        if (flags & 2 != 0) != has_pawns {
            return None;
        }
        let symmetric = flags & 1 == 0;
        let files = if has_pawns { 4 } else { 1 };
        let sides = if kind == Kind::Wdl && !symmetric {
            2
        } else {
            1
        };

        // Descritores de peças (1.5).
        let mut at = 5;
        let mut described = Vec::new(); // (coluna, lado) -> (peças, order, order2)
        for f in 0..files {
            let order = data[at];
            at += 1;
            let order2 = if both_pawns {
                at += 1;
                data[at - 1]
            } else {
                0xFF
            };
            let bytes = data.get(at..at + pieces_count)?;
            at += pieces_count;
            for side in 0..sides {
                let shift = 4 * side as u8;
                let pieces: Vec<u8> = bytes.iter().map(|b| (b >> shift) & 0xF).collect();
                let o = (order >> shift) & 0xF;
                let o2 = if order2 == 0xFF {
                    15
                } else {
                    (order2 >> shift) & 0xF
                };
                described.push((f, pieces, o, o2));
            }
        }
        at += at & 1;

        // Cabeçalhos de compressão (4.1), coluna de fora, lado de dentro.
        let mut sub = Vec::new();
        for (f, pieces, order, order2) in described {
            let (groups, size) = encode::groups(&pieces, order, order2, has_pawns, f);
            let (pairs, next) = Pairs::parse_header(&data, at, kind)?;
            at = next;
            sub.push(SubTable {
                pieces,
                groups,
                size,
                pairs,
            });
        }

        // Mapas DTZ (5.3).
        let mut maps = vec![None; files];
        if kind == Kind::Dtz {
            for (f, map) in maps.iter_mut().enumerate() {
                let flags = sub[f].pairs.flags;
                if flags & 2 == 0 || sub[f].pairs.single.is_some() {
                    continue;
                }
                let wide = flags & 16 != 0;
                if wide {
                    at += at & 1;
                }
                let mut offsets = [0; 4];
                for slot in offsets.iter_mut() {
                    *slot = at;
                    let n = if wide {
                        usize::from(u16_le(&data, at)?)
                    } else {
                        usize::from(*data.get(at)?)
                    };
                    at += if wide { 2 + 2 * n } else { 1 + n };
                }
                *map = Some(DtzMap { offsets, wide });
            }
            at += at & 1;
        }

        for s in &mut sub {
            s.pairs.index_offset = at;
            if s.pairs.single.is_none() {
                at += 6 * s.size.div_ceil(1u64 << s.pairs.idx_bits) as usize;
            }
        }
        for s in &mut sub {
            s.pairs.sizes_offset = at;
            if s.pairs.single.is_none() {
                at += 2 * s.pairs.size_entries as usize;
            }
        }
        for s in &mut sub {
            at = at.next_multiple_of(64);
            s.pairs.data_offset = at;
            if s.pairs.single.is_none() {
                at += (s.pairs.real_blocks as usize) << s.pairs.block_size;
            }
        }
        if at > data.len() {
            return None;
        }
        Some(Table {
            data,
            kind,
            has_pawns,
            symmetric,
            sub,
            sides,
            maps,
        })
    }

    pub fn subtable(&self, file: usize, side: usize) -> &SubTable {
        &self.sub[file * self.sides + side]
    }

    /// Valor bruto guardado no índice `idx` da sub-tabela.
    pub fn value(&self, sub: &SubTable, idx: u64) -> Option<u16> {
        sub.pairs.decode(&self.data, idx)
    }

    /// Valor do mapa DTZ da coluna `file` para a classe `class` (0 vitória, 1 derrota,
    /// 2 amaldiçoada, 3 abençoada).
    pub fn map_value(&self, file: usize, class: usize, value: u16) -> Option<u16> {
        let map = self.maps.get(file).copied().flatten()?;
        let at = map.offsets[class];
        if map.wide {
            u16_le(&self.data, at + 2 + 2 * usize::from(value))
        } else {
            self.data
                .get(at + 1 + usize::from(value))
                .map(|&b| u16::from(b))
        }
    }
}

impl Pairs {
    fn parse_header(d: &[u8], at: usize, kind: Kind) -> Option<(Pairs, usize)> {
        let flags = *d.get(at)?;
        let empty = |single| Pairs {
            flags,
            single,
            block_size: 0,
            idx_bits: 0,
            real_blocks: 0,
            size_entries: 0,
            min_len: 0,
            lowest: Vec::new(),
            base: Vec::new(),
            symbols: Vec::new(),
            symlen: Vec::new(),
            index_offset: 0,
            sizes_offset: 0,
            data_offset: 0,
        };
        if flags & 0x80 != 0 {
            // Em DTZ o valor único é sempre 0 (seção 4.1).
            let value = if kind == Kind::Wdl {
                u16::from(*d.get(at + 1)?)
            } else {
                0
            };
            return Some((empty(Some(value)), at + 2));
        }
        let block_size = u32::from(*d.get(at + 1)?);
        let idx_bits = u32::from(*d.get(at + 2)?);
        let padding = u32::from(*d.get(at + 3)?);
        let real_blocks = u32_le(d, at + 4)?;
        let max_len = u32::from(*d.get(at + 8)?);
        let min_len = u32::from(*d.get(at + 9)?);
        if min_len == 0 || max_len < min_len || max_len >= 64 {
            return None;
        }
        let h = (max_len - min_len + 1) as usize;
        let lowest: Vec<u16> = (0..h)
            .map(|i| u16_le(d, at + 10 + 2 * i))
            .collect::<Option<_>>()?;
        let mut cursor = at + 10 + 2 * h;
        let num_syms = usize::from(u16_le(d, cursor)?);
        cursor += 2;
        let symbols: Vec<[u8; 3]> = (0..num_syms)
            .map(|i| {
                d.get(cursor + 3 * i..cursor + 3 * i + 3)
                    .map(|b| [b[0], b[1], b[2]])
            })
            .collect::<Option<_>>()?;
        cursor += 3 * num_syms + (num_syms & 1);

        // Base do Huffman canônico (4.3).
        let mut b = vec![0i64; h];
        for i in (0..h.saturating_sub(1)).rev() {
            b[i] = (b[i + 1] + i64::from(lowest[i]) - i64::from(lowest[i + 1])) / 2;
            if 2 * b[i] < b[i + 1] {
                return None;
            }
        }
        let base = (0..h)
            .map(|i| {
                (b[i] as u64)
                    .checked_shl(64 - (min_len + i as u32))
                    .unwrap_or(0)
            })
            .collect();

        let mut pairs = Pairs {
            block_size,
            idx_bits,
            real_blocks,
            size_entries: real_blocks + padding,
            min_len,
            lowest,
            base,
            symbols,
            ..empty(None)
        };
        pairs.symlen = pairs.compute_symlen()?;
        Some((pairs, cursor))
    }

    fn left(&self, s: usize) -> usize {
        let b = self.symbols[s];
        usize::from(b[0]) | usize::from(b[1] & 0x0F) << 8
    }

    fn right(&self, s: usize) -> usize {
        let b = self.symbols[s];
        usize::from(b[1] >> 4) | usize::from(b[2]) << 4
    }

    /// symlen[s] = número de valores representados por s, menos 1 (4.2). Rejeita ciclos.
    fn compute_symlen(&self) -> Option<Vec<u32>> {
        const UNKNOWN: u32 = u32::MAX;
        const VISITING: u32 = u32::MAX - 1;
        let n = self.symbols.len();
        let mut len = vec![UNKNOWN; n];
        for root in 0..n {
            let mut stack = vec![root];
            while let Some(&s) = stack.last() {
                if len[s] != UNKNOWN && len[s] != VISITING {
                    stack.pop();
                    continue;
                }
                if self.right(s) == 0xFFF {
                    len[s] = 0;
                    stack.pop();
                    continue;
                }
                let (l, r) = (self.left(s), self.right(s));
                if l >= n || r >= n {
                    return None;
                }
                let ready = |len: &[u32], x: usize| len[x] != UNKNOWN && len[x] != VISITING;
                if ready(&len, l) && ready(&len, r) {
                    len[s] = len[l] + len[r] + 1;
                    stack.pop();
                } else {
                    if len[s] == VISITING {
                        return None; // ciclo
                    }
                    len[s] = VISITING;
                    for x in [l, r] {
                        if !ready(&len, x) {
                            if len[x] == VISITING {
                                return None;
                            }
                            stack.push(x);
                        }
                    }
                }
            }
        }
        Some(len)
    }

    /// Valor do índice `idx` (4.5).
    fn decode(&self, d: &[u8], idx: u64) -> Option<u16> {
        if let Some(v) = self.single {
            return Some(v);
        }
        let span = 1u64 << self.idx_bits;
        let entry = self.index_offset + 6 * (idx / span) as usize;
        let mut block = u32_le(d, entry)? as i64;
        let offset = i64::from(u16_le(d, entry + 4)?);
        let mut lit = offset + (idx % span) as i64 - (span / 2) as i64;
        let size_of = |b: i64| -> Option<i64> {
            if b < 0 || b >= i64::from(self.size_entries) {
                return None;
            }
            Some(i64::from(u16_le(d, self.sizes_offset + 2 * b as usize)?))
        };
        while lit < 0 {
            block -= 1;
            lit += size_of(block)? + 1;
        }
        while lit > size_of(block)? {
            lit -= size_of(block)? + 1;
            block += 1;
        }
        if block >= i64::from(self.real_blocks) {
            return None;
        }
        let mut ptr = self.data_offset + ((block as usize) << self.block_size);
        let mut window = be(d, ptr, 8);
        ptr += 8;
        let mut consumed = 0u32;
        let mut lit = lit as u64;
        let sym = loop {
            let mut l = 0usize;
            while l + 1 < self.base.len() && window < self.base[l] {
                l += 1;
            }
            let len = self.min_len + l as u32;
            let sym =
                usize::from(self.lowest[l]) + ((window - self.base[l]) >> (64 - len)) as usize;
            let count = u64::from(*self.symlen.get(sym)?) + 1;
            if lit < count {
                break sym;
            }
            lit -= count;
            window <<= len;
            consumed += len;
            if consumed >= 32 {
                consumed -= 32;
                window |= be(d, ptr, 4) << consumed;
                ptr += 4;
            }
        };
        let mut sym = sym;
        while self.symlen[sym] > 0 {
            let l = self.left(sym);
            let count = u64::from(self.symlen[l]) + 1;
            if lit < count {
                sym = l;
            } else {
                lit -= count;
                sym = self.right(sym);
            }
        }
        Some(self.left(sym) as u16)
    }
}
