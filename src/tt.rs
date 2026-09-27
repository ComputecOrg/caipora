//! Tabela de transposição: guarda, por hash de posição, o melhor lance e o resultado de buscas
//! anteriores. As entradas ficam em grupos de 4 (uma linha de cache); cada chave cai num grupo e
//! pode ocupar qualquer entrada dele. A chave completa descarta colisões.
//!
//! A tabela é compartilhada entre threads sem trava: cada entrada são dois inteiros atômicos, os
//! dados empacotados e a chave combinada com eles por XOR. Se dois threads gravarem a mesma
//! entrada ao mesmo tempo, a mistura não passa na conferência da chave e é ignorada.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering::Relaxed};

use crate::moves::Move;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    /// O valor é exato (a busca ficou dentro da janela).
    Exact,
    /// O valor é no mínimo este (houve corte beta).
    Lower,
    /// O valor é no máximo este (nenhum lance superou alpha).
    Upper,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TtEntry {
    pub mv: Option<Move>,
    pub score: i32,
    pub depth: i32,
    pub bound: Bound,
}

/// Conteúdo de uma entrada, desempacotado.
#[derive(Clone, Copy)]
struct Data {
    mv: Option<Move>,
    score: i16,
    depth: i8,
    /// 0 = vazio; 1, 2, 3 = `Bound`.
    bound: u8,
    /// Busca em que a entrada foi gravada (conta módulo 256).
    generation: u8,
}

impl Data {
    /// Bits 0–15 lance, 16–31 pontuação, 32–39 profundidade, 40–47 limite, 48–55 geração.
    fn pack(self) -> u64 {
        u64::from(self.mv.map_or(0, Move::to_bits))
            | u64::from(self.score as u16) << 16
            | u64::from(self.depth as u8) << 32
            | u64::from(self.bound) << 40
            | u64::from(self.generation) << 48
    }

    fn unpack(bits: u64) -> Data {
        Data {
            mv: Move::from_bits(bits as u16),
            score: (bits >> 16) as u16 as i16,
            depth: (bits >> 32) as u8 as i8,
            bound: (bits >> 40) as u8,
            generation: (bits >> 48) as u8,
        }
    }
}

#[derive(Default)]
struct Slot {
    /// Chave XOR dados: só confere com a chave procurada se as duas metades são da mesma gravação.
    key: AtomicU64,
    data: AtomicU64,
}

impl Slot {
    fn load(&self) -> (u64, Data) {
        let data = self.data.load(Relaxed);
        (self.key.load(Relaxed) ^ data, Data::unpack(data))
    }

    fn save(&self, key: u64, data: Data) {
        let bits = data.pack();
        self.key.store(key ^ bits, Relaxed);
        self.data.store(bits, Relaxed);
    }
}

const CLUSTER_SIZE: usize = 4;

#[derive(Default)]
#[repr(C, align(64))]
struct Cluster {
    slots: [Slot; CLUSTER_SIZE],
}

/// Quanto vale manter uma entrada: profundidade, descontada pela idade (em buscas).
const AGE_WEIGHT: i32 = 8;

pub struct TranspositionTable {
    clusters: Vec<Cluster>,
    generation: AtomicU8,
}

impl TranspositionTable {
    pub fn new(megabytes: usize) -> TranspositionTable {
        let count = (megabytes.max(1) * 1024 * 1024 / std::mem::size_of::<Cluster>()).max(1);
        TranspositionTable {
            clusters: (0..count).map(|_| Cluster::default()).collect(),
            generation: AtomicU8::new(0),
        }
    }

    pub fn clear(&mut self) {
        for slot in self.clusters.iter_mut().flat_map(|c| c.slots.iter_mut()) {
            *slot.key.get_mut() = 0;
            *slot.data.get_mut() = 0;
        }
        *self.generation.get_mut() = 0;
    }

    /// Começo de uma nova busca: as entradas atuais passam a ser "da busca anterior".
    pub fn new_search(&self) {
        self.generation.fetch_add(1, Relaxed);
    }

    /// Mapeia a chave para `0..len` pela parte alta do produto, sem exigir potência de 2.
    fn index(&self, key: u64) -> usize {
        ((u128::from(key) * self.clusters.len() as u128) >> 64) as usize
    }

    pub fn probe(&self, key: u64) -> Option<TtEntry> {
        let cluster = &self.clusters[self.index(key)];
        let data = cluster.slots.iter().find_map(|slot| {
            let (stored, data) = slot.load();
            (data.bound != 0 && stored == key).then_some(data)
        })?;
        let bound = match data.bound {
            1 => Bound::Exact,
            2 => Bound::Lower,
            _ => Bound::Upper,
        };
        Some(TtEntry {
            mv: data.mv,
            score: i32::from(data.score),
            depth: i32::from(data.depth),
            bound,
        })
    }

    /// Grava na entrada da mesma posição, se já existir no grupo; senão, numa vazia; senão, na que
    /// vale menos (rasa ou velha). Preserva o lance anterior da mesma posição quando a nova busca
    /// não achou um.
    pub fn store(&self, key: u64, mv: Option<Move>, score: i32, depth: i32, bound: Bound) {
        let generation = self.generation.load(Relaxed);
        let cluster = &self.clusters[self.index(key)];
        let loaded: [(u64, Data); CLUSTER_SIZE] = std::array::from_fn(|i| cluster.slots[i].load());
        let target = loaded
            .iter()
            .position(|(stored, data)| data.bound != 0 && *stored == key)
            .or_else(|| loaded.iter().position(|(_, data)| data.bound == 0))
            .unwrap_or_else(|| {
                (0..CLUSTER_SIZE)
                    .min_by_key(|&i| worth(&loaded[i].1, generation))
                    .expect("grupo não vazio")
            });
        let (stored, previous) = loaded[target];
        let mv = if mv.is_none() && previous.bound != 0 && stored == key {
            previous.mv
        } else {
            mv
        };
        cluster.slots[target].save(
            key,
            Data {
                mv,
                score: score.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
                depth: depth.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8,
                bound: match bound {
                    Bound::Exact => 1,
                    Bound::Lower => 2,
                    Bound::Upper => 3,
                },
                generation,
            },
        );
    }

    /// Ocupação em milésimos, estimada pelas primeiras 1000 entradas (campo `hashfull` do UCI).
    pub fn hashfull(&self) -> u32 {
        let sample = self
            .clusters
            .iter()
            .flat_map(|cluster| cluster.slots.iter())
            .take(1000);
        let (used, total) = sample.fold((0, 0), |(used, total), slot| {
            (used + usize::from(slot.load().1.bound != 0), total + 1)
        });
        (used * 1000 / total) as u32
    }

    /// Número de entradas.
    pub fn len(&self) -> usize {
        self.clusters.len() * CLUSTER_SIZE
    }

    pub fn is_empty(&self) -> bool {
        self.clusters.is_empty()
    }
}

/// Valor de manter a entrada: a profundidade, menos `AGE_WEIGHT` por busca de idade.
fn worth(data: &Data, generation: u8) -> i32 {
    let age = i32::from(generation.wrapping_sub(data.generation));
    i32::from(data.depth) - AGE_WEIGHT * age
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moves::MoveKind;

    fn mv(from: &str, to: &str) -> Move {
        Move::new(from.parse().unwrap(), to.parse().unwrap(), MoveKind::Normal)
    }

    #[test]
    fn size_follows_megabytes() {
        let tt = TranspositionTable::new(1);
        assert_eq!(tt.len(), 1024 * 1024 / std::mem::size_of::<Slot>());
        assert_eq!(std::mem::size_of::<Slot>(), 16);
    }

    #[test]
    fn stored_entry_is_found_only_under_its_key() {
        let tt = TranspositionTable::new(1);
        let key = 0xDEAD_BEEF_1234_5678;
        assert_eq!(tt.probe(key), None);
        tt.store(key, Some(mv("e2", "e4")), -123, 7, Bound::Lower);
        assert_eq!(
            tt.probe(key),
            Some(TtEntry {
                mv: Some(mv("e2", "e4")),
                score: -123,
                depth: 7,
                bound: Bound::Lower,
            })
        );
        assert_eq!(tt.probe(key ^ 1), None);
    }

    #[test]
    fn a_store_without_move_keeps_the_previous_move() {
        let tt = TranspositionTable::new(1);
        tt.store(42, Some(mv("g1", "f3")), 10, 3, Bound::Exact);
        tt.store(42, None, 5, 4, Bound::Upper);
        let entry = tt.probe(42).unwrap();
        assert_eq!(entry.mv, Some(mv("g1", "f3")));
        assert_eq!(
            (entry.score, entry.depth, entry.bound),
            (5, 4, Bound::Upper)
        );
    }

    /// Chaves que caem todas no mesmo grupo: a parte alta de `chave × grupos` é a mesma.
    fn same_cluster(i: u64) -> u64 {
        0x8000_0000_0000_0000 + i
    }

    #[test]
    fn a_cluster_holds_four_entries_in_one_cache_line() {
        assert_eq!(std::mem::size_of::<Cluster>(), 64);
        assert_eq!(std::mem::align_of::<Cluster>(), 64);
        let tt = TranspositionTable::new(1);
        for i in 0..4 {
            tt.store(same_cluster(i), None, i as i32, 3, Bound::Exact);
        }
        for i in 0..4 {
            assert_eq!(tt.probe(same_cluster(i)).map(|e| e.score), Some(i as i32));
        }
    }

    #[test]
    fn a_deep_entry_survives_shallow_stores_to_its_cluster() {
        let tt = TranspositionTable::new(1);
        tt.store(same_cluster(0), Some(mv("e2", "e4")), 50, 12, Bound::Exact);
        for i in 1..40 {
            tt.store(same_cluster(i), None, 0, 1, Bound::Upper);
        }
        assert_eq!(tt.probe(same_cluster(0)).map(|e| e.depth), Some(12));
    }

    #[test]
    fn entries_from_older_searches_are_replaced_first() {
        let tt = TranspositionTable::new(1);
        for i in 0..4 {
            tt.store(same_cluster(i), None, 0, 5, Bound::Exact);
        }
        tt.new_search();
        for i in 4..8 {
            tt.store(same_cluster(i), None, 0, 1, Bound::Exact);
        }
        for i in 0..4 {
            assert_eq!(tt.probe(same_cluster(i)), None, "antiga {i}");
        }
        for i in 4..8 {
            assert!(tt.probe(same_cluster(i)).is_some(), "nova {i}");
        }
    }

    #[test]
    fn the_same_position_is_updated_in_place() {
        let tt = TranspositionTable::new(1);
        tt.store(same_cluster(0), None, 10, 6, Bound::Lower);
        tt.store(same_cluster(0), None, 20, 2, Bound::Upper);
        for i in 1..4 {
            tt.store(same_cluster(i), None, 0, 9, Bound::Exact);
        }
        let entry = tt.probe(same_cluster(0)).unwrap();
        assert_eq!(
            (entry.score, entry.depth, entry.bound),
            (20, 2, Bound::Upper)
        );
        for i in 1..4 {
            assert!(tt.probe(same_cluster(i)).is_some());
        }
    }

    #[test]
    fn concurrent_writers_never_produce_a_mixed_entry() {
        // Quatro threads gravam e leem chaves do mesmo grupo ao mesmo tempo. Cada chave tem um
        // conteúdo fixo, derivado dela; uma leitura só pode devolver esse conteúdo ou nada.
        let tt = TranspositionTable::new(1);
        let content = |key: u64| ((key % 997) as i32 - 400, (key % 40) as i32 + 1);
        std::thread::scope(|scope| {
            for thread in 0..4u64 {
                let tt = &tt;
                scope.spawn(move || {
                    for i in 0..50_000u64 {
                        let key = same_cluster((i * 7 + thread) % 12);
                        let (score, depth) = content(key);
                        tt.store(key, Some(mv("g1", "f3")), score, depth, Bound::Exact);
                        let probed = same_cluster((i * 3 + thread + 1) % 12);
                        if let Some(entry) = tt.probe(probed) {
                            assert_eq!((entry.score, entry.depth), content(probed), "{probed:x}");
                            assert_eq!(entry.mv, Some(mv("g1", "f3")));
                        }
                    }
                });
            }
        });
    }

    #[test]
    fn clear_and_hashfull() {
        let mut tt = TranspositionTable::new(1);
        assert_eq!(tt.hashfull(), 0);
        for key in 0..tt.len() as u64 * 4 {
            tt.store(
                key.wrapping_mul(0x9E37_79B9_7F4A_7C15),
                None,
                0,
                1,
                Bound::Exact,
            );
        }
        assert!(tt.hashfull() > 900);
        tt.clear();
        assert_eq!(tt.hashfull(), 0);
    }
}
