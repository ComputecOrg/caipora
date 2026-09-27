//! Tabela de transposição: guarda, por hash de posição, o melhor lance e o resultado de buscas
//! anteriores. Uma entrada por posição da tabela, com a chave completa para descartar colisões.

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
}

pub struct TranspositionTable {
    slots: Vec<Slot>,
}

impl TranspositionTable {
    pub fn new(megabytes: usize) -> TranspositionTable {
        let count = (megabytes.max(1) * 1024 * 1024 / std::mem::size_of::<Slot>()).max(1);
        TranspositionTable {
            slots: vec![Slot::default(); count],
        }
    }

    pub fn clear(&mut self) {
        self.slots.fill(Slot::default());
    }

    /// Mapeia a chave para `0..len` pela parte alta do produto, sem exigir potência de 2.
    fn index(&self, key: u64) -> usize {
        ((u128::from(key) * self.slots.len() as u128) >> 64) as usize
    }

    pub fn probe(&self, key: u64) -> Option<TtEntry> {
        let slot = self.slots[self.index(key)];
        let bound = match slot.bound {
            1 => Bound::Exact,
            2 => Bound::Lower,
            3 => Bound::Upper,
            _ => return None,
        };
        (slot.key == key).then_some(TtEntry {
            mv: slot.mv,
            score: i32::from(slot.score),
            depth: i32::from(slot.depth),
            bound,
        })
    }

    /// Substitui sempre (política simples da v1), mas preserva o lance anterior da mesma posição
    /// quando a nova busca não achou um.
    pub fn store(&mut self, key: u64, mv: Option<Move>, score: i32, depth: i32, bound: Bound) {
        let index = self.index(key);
        let slot = &mut self.slots[index];
        let mv = if mv.is_none() && slot.key == key {
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
        };
    }

    /// Ocupação em milésimos, estimada pelas primeiras 1000 posições (campo `hashfull` do UCI).
    pub fn hashfull(&self) -> u32 {
        let sample = &self.slots[..self.slots.len().min(1000)];
        let used = sample.iter().filter(|slot| slot.bound != 0).count();
        (used * 1000 / sample.len()) as u32
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
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
