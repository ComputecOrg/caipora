//! Tabela de transposição: guarda, por hash de posição, o melhor lance e o resultado de buscas
//! anteriores. As entradas ficam em grupos de 4 (uma linha de cache); cada chave cai num grupo e
//! pode ocupar qualquer entrada dele. A chave completa descarta colisões.

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

#[derive(Clone, Copy, Default)]
struct Slot {
    key: u64,
    mv: Option<Move>,
    score: i16,
    depth: i8,
    /// 0 = vazio; 1, 2, 3 = `Bound`.
    bound: u8,
    /// Busca em que a entrada foi gravada (conta módulo 256).
    generation: u8,
}

const CLUSTER_SIZE: usize = 4;

#[derive(Clone, Copy, Default)]
#[repr(C, align(64))]
struct Cluster {
    slots: [Slot; CLUSTER_SIZE],
}

/// Quanto vale manter uma entrada: profundidade, descontada pela idade (em buscas).
const AGE_WEIGHT: i32 = 8;

pub struct TranspositionTable {
    clusters: Vec<Cluster>,
    generation: u8,
}

impl TranspositionTable {
    pub fn new(megabytes: usize) -> TranspositionTable {
        let count = (megabytes.max(1) * 1024 * 1024 / std::mem::size_of::<Cluster>()).max(1);
        TranspositionTable {
            clusters: vec![Cluster::default(); count],
            generation: 0,
        }
    }

    pub fn clear(&mut self) {
        self.clusters.fill(Cluster::default());
        self.generation = 0;
    }

    /// Começo de uma nova busca: as entradas atuais passam a ser "da busca anterior".
    pub fn new_search(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Mapeia a chave para `0..len` pela parte alta do produto, sem exigir potência de 2.
    fn index(&self, key: u64) -> usize {
        ((u128::from(key) * self.clusters.len() as u128) >> 64) as usize
    }

    pub fn probe(&self, key: u64) -> Option<TtEntry> {
        let cluster = &self.clusters[self.index(key)];
        let slot = cluster
            .slots
            .iter()
            .find(|slot| slot.bound != 0 && slot.key == key)?;
        let bound = match slot.bound {
            1 => Bound::Exact,
            2 => Bound::Lower,
            _ => Bound::Upper,
        };
        Some(TtEntry {
            mv: slot.mv,
            score: i32::from(slot.score),
            depth: i32::from(slot.depth),
            bound,
        })
    }

    /// Grava na entrada da mesma posição, se já existir no grupo; senão, numa vazia; senão, na que
    /// vale menos (rasa ou velha). Preserva o lance anterior da mesma posição quando a nova busca
    /// não achou um.
    pub fn store(&mut self, key: u64, mv: Option<Move>, score: i32, depth: i32, bound: Bound) {
        let generation = self.generation;
        let index = self.index(key);
        let cluster = &mut self.clusters[index];
        let target = cluster
            .slots
            .iter()
            .position(|slot| slot.bound != 0 && slot.key == key)
            .or_else(|| cluster.slots.iter().position(|slot| slot.bound == 0))
            .unwrap_or_else(|| {
                (0..CLUSTER_SIZE)
                    .min_by_key(|&i| worth(&cluster.slots[i], generation))
                    .expect("grupo não vazio")
            });
        let slot = &mut cluster.slots[target];
        let mv = if mv.is_none() && slot.bound != 0 && slot.key == key {
            slot.mv
        } else {
            mv
        };
        *slot = Slot {
            key,
            mv,
            score: score.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
            depth: depth.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8,
            bound: match bound {
                Bound::Exact => 1,
                Bound::Lower => 2,
                Bound::Upper => 3,
            },
            generation,
        };
    }

    /// Ocupação em milésimos, estimada pelas primeiras 1000 entradas (campo `hashfull` do UCI).
    pub fn hashfull(&self) -> u32 {
        let sample = self
            .clusters
            .iter()
            .flat_map(|cluster| cluster.slots.iter())
            .take(1000);
        let (used, total) = sample.fold((0, 0), |(used, total), slot| {
            (used + usize::from(slot.bound != 0), total + 1)
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
fn worth(slot: &Slot, generation: u8) -> i32 {
    let age = i32::from(generation.wrapping_sub(slot.generation));
    i32::from(slot.depth) - AGE_WEIGHT * age
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
        let mut tt = TranspositionTable::new(1);
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
        let mut tt = TranspositionTable::new(1);
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
        let mut tt = TranspositionTable::new(1);
        for i in 0..4 {
            tt.store(same_cluster(i), None, i as i32, 3, Bound::Exact);
        }
        for i in 0..4 {
            assert_eq!(tt.probe(same_cluster(i)).map(|e| e.score), Some(i as i32));
        }
    }

    #[test]
    fn a_deep_entry_survives_shallow_stores_to_its_cluster() {
        let mut tt = TranspositionTable::new(1);
        tt.store(same_cluster(0), Some(mv("e2", "e4")), 50, 12, Bound::Exact);
        for i in 1..40 {
            tt.store(same_cluster(i), None, 0, 1, Bound::Upper);
        }
        assert_eq!(tt.probe(same_cluster(0)).map(|e| e.depth), Some(12));
    }

    #[test]
    fn entries_from_older_searches_are_replaced_first() {
        let mut tt = TranspositionTable::new(1);
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
        let mut tt = TranspositionTable::new(1);
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
