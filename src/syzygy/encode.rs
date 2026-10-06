//! Índice de uma posição dentro de uma sub-tabela Syzygy (docs/syzygy-spec.md, seção 3).
//!
//! Todas as tabelas auxiliares são calculadas a partir das suas definições na especificação.

use std::sync::LazyLock;

/// Quantas posições o grupo líder sem peões tem com 3 peças únicas e com só os 2 reis.
pub const LEADER_THREE: u64 = 31_332;
pub const LEADER_KINGS: u64 = 462;

const fn file(sq: usize) -> i32 {
    (sq % 8) as i32
}

const fn rank(sq: usize) -> i32 {
    (sq / 8) as i32
}

/// fileira − coluna: 0 na diagonal a1–h8, negativo abaixo (lado de h1), positivo acima.
pub const fn off_diagonal(sq: usize) -> i32 {
    rank(sq) - file(sq)
}

pub const fn mirror_horizontal(sq: usize) -> usize {
    sq ^ 7
}

pub const fn mirror_vertical(sq: usize) -> usize {
    sq ^ 56
}

pub const fn mirror_diagonal(sq: usize) -> usize {
    (file(sq) * 8 + rank(sq)) as usize
}

/// Distância da coluna à borda (0 = a/h … 3 = d/e).
pub const fn edge_distance(sq: usize) -> usize {
    let f = file(sq);
    (if f < 4 { f } else { 7 - f }) as usize
}

pub struct Tables {
    /// C(n, k) para n < 65 e k < 8.
    binomial: [[u64; 8]; 65],
    /// Casas do triângulo a1–d1–d4: abaixo da diagonal 0…5, na diagonal 6…9.
    tri: [Option<u8>; 64],
    /// Casas abaixo da diagonal, numeradas 0…27 em ordem crescente.
    lower: [Option<u8>; 64],
    /// Os dois reis (rei do triângulo × outro rei): 0…461.
    kk: [[Option<u16>; 64]; 10],
    /// Ordem dos peões líderes ("pawn twist"), definida nas fileiras 2 a 7.
    pawn_twist: [u8; 64],
    /// Primeiro índice do grupo de c peões líderes com o mais à borda na casa s (colunas a–d).
    lead_start: [[u64; 64]; 6],
    /// Número de posições do grupo de c peões líderes, por coluna a–d.
    lead_size: [[u64; 4]; 6],
}

pub static TABLES: LazyLock<Tables> = LazyLock::new(Tables::build);

impl Tables {
    fn build() -> Tables {
        let mut binomial = [[0u64; 8]; 65];
        for n in 0..65 {
            binomial[n][0] = 1;
            for k in 1..8 {
                if n > 0 {
                    binomial[n][k] = binomial[n - 1][k - 1] + binomial[n - 1][k];
                }
            }
        }

        let mut tri = [None; 64];
        let below = [1usize, 2, 3, 10, 11, 19]; // b1, c1, d1, c2, d2, d3
        let diagonal = [0usize, 9, 18, 27]; // a1, b2, c3, d4
        for (i, &sq) in below.iter().chain(diagonal.iter()).enumerate() {
            tri[sq] = Some(i as u8);
        }
        debug_assert!(below.iter().all(|&s| off_diagonal(s) < 0 && file(s) < 4));

        let mut lower = [None; 64];
        let mut next = 0u8;
        for (sq, slot) in lower.iter_mut().enumerate() {
            if off_diagonal(sq) < 0 {
                *slot = Some(next);
                next += 1;
            }
        }

        let mut kk = [[None; 64]; 10];
        let mut count = 0u16;
        let mut deferred = Vec::new();
        for (t, row) in kk.iter_mut().enumerate() {
            let s1 = tri.iter().position(|&v| v == Some(t as u8)).unwrap();
            for (s2, slot) in row.iter_mut().enumerate() {
                let adjacent = (file(s1) - file(s2)).abs() <= 1 && (rank(s1) - rank(s2)).abs() <= 1;
                if adjacent {
                    continue;
                }
                let s1_on_diagonal = off_diagonal(s1) == 0;
                if s1_on_diagonal && off_diagonal(s2) > 0 {
                    continue;
                }
                if s1_on_diagonal && off_diagonal(s2) == 0 {
                    deferred.push((t, s2));
                    continue;
                }
                *slot = Some(count);
                count += 1;
            }
        }
        for (t, s2) in deferred {
            kk[t][s2] = Some(count);
            count += 1;
        }
        debug_assert_eq!(count as u64, LEADER_KINGS);

        let mut pawn_twist = [0u8; 64];
        for (sq, slot) in pawn_twist.iter_mut().enumerate() {
            let r = rank(sq);
            if (1..=6).contains(&r) {
                let e = edge_distance(sq) as i32;
                *slot = (47 - 2 * (6 * e + r - 1) - i32::from(file(sq) >= 4)) as u8;
            }
        }

        let mut lead_start = [[0u64; 64]; 6];
        let mut lead_size = [[0u64; 4]; 6];
        for c in 1..6 {
            for (f, size) in lead_size[c].iter_mut().enumerate() {
                let mut sum = 0;
                for r in 1..=6 {
                    let sq = r * 8 + f;
                    lead_start[c][sq] = sum;
                    sum += binomial[pawn_twist[sq] as usize][c - 1];
                }
                *size = sum;
            }
        }

        Tables {
            binomial,
            tri,
            lower,
            kk,
            pawn_twist,
            lead_start,
            lead_size,
        }
    }

    pub fn binomial(&self, n: i64, k: usize) -> u64 {
        if !(0..=64).contains(&n) || k > 7 {
            0
        } else {
            self.binomial[n as usize][k]
        }
    }

    pub fn pawn_twist(&self, sq: usize) -> u8 {
        self.pawn_twist[sq]
    }

    pub fn lead_size(&self, pawns: usize, file: usize) -> u64 {
        self.lead_size[pawns][file]
    }

    /// Índice do grupo líder sem peões com 3 peças únicas (casas já normalizadas).
    fn leader_three(&self, p: &[usize]) -> u64 {
        let (p0, p1, p2) = (p[0], p[1], p[2]);
        let a1 = u64::from(p1 > p0);
        let a2 = u64::from(p2 > p0) + u64::from(p2 > p1);
        let d = |s: usize| rank(s) as u64;
        let lower = |s: usize| u64::from(self.lower[s].expect("casa abaixo da diagonal"));
        if off_diagonal(p0) != 0 {
            u64::from(self.tri[p0].expect("casa do triângulo")) * 63 * 62
                + (p1 as u64 - a1) * 62
                + (p2 as u64 - a2)
        } else if off_diagonal(p1) != 0 {
            6 * 63 * 62 + (d(p0) * 28 + lower(p1)) * 62 + (p2 as u64 - a2)
        } else if off_diagonal(p2) != 0 {
            6 * 63 * 62 + 4 * 28 * 62 + d(p0) * 7 * 28 + (d(p1) - a1) * 28 + lower(p2)
        } else {
            6 * 63 * 62 + 4 * 28 * 62 + 4 * 7 * 28 + d(p0) * 7 * 6 + (d(p1) - a1) * 6 + (d(p2) - a2)
        }
    }

    fn leader_kings(&self, p: &[usize]) -> Option<u64> {
        let t = self.tri[p[0]]? as usize;
        self.kk[t][p[1]].map(u64::from)
    }
}

/// Tipo de grupo de codificação.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKind {
    Leader,
    RemainingPawns,
    Other,
}

#[derive(Clone, Debug)]
pub struct Group {
    pub start: usize,
    pub len: usize,
    pub kind: GroupKind,
    /// Número de maneiras de colocar o grupo.
    pub size: u64,
    /// Peso do índice local no índice da sub-tabela.
    pub factor: u64,
}

/// Grupos, fatores e tamanho de uma sub-tabela a partir da sequência de peças (códigos de 4
/// bits), do `order`/`order2` e da coluna do peão líder (`file`, só com peões).
pub fn groups(
    pieces: &[u8],
    order: u8,
    order2: u8,
    has_pawns: bool,
    file: usize,
) -> (Vec<Group>, u64) {
    let t = &*TABLES;
    let n = pieces.len();
    let mut groups = Vec::new();
    let mut i;
    if has_pawns {
        let lead = pieces.iter().take_while(|&&c| c == pieces[0]).count();
        groups.push(Group {
            start: 0,
            len: lead,
            kind: GroupKind::Leader,
            size: t.lead_size(lead, file),
            factor: 0,
        });
        i = lead;
        let rest = pieces[i..].iter().take_while(|&&c| c & 7 == 1).count();
        if rest > 0 {
            let size = t.binomial(48 - lead as i64, rest);
            groups.push(Group {
                start: i,
                len: rest,
                kind: GroupKind::RemainingPawns,
                size,
                factor: 0,
            });
            i += rest;
        }
    } else {
        let unique = pieces
            .iter()
            .filter(|&&c| pieces.iter().filter(|&&d| d == c).count() == 1)
            .count();
        let (len, size) = if unique >= 3 {
            (3, LEADER_THREE)
        } else {
            (2, LEADER_KINGS)
        };
        groups.push(Group {
            start: 0,
            len,
            kind: GroupKind::Leader,
            size,
            factor: 0,
        });
        i = len;
    }
    let mut free = 64 - i as i64;
    while i < n {
        let len = pieces[i..].iter().take_while(|&&c| c == pieces[i]).count();
        let size = t.binomial(free, len);
        groups.push(Group {
            start: i,
            len,
            kind: GroupKind::Other,
            size,
            factor: 0,
        });
        free -= len as i64;
        i += len;
    }

    // Ordem de significância: o líder vai na posição `order`, os peões restantes em `order2`,
    // os demais preenchem as posições livres na ordem do cabeçalho.
    let count = groups.len();
    let mut slots: Vec<Option<usize>> = vec![None; count];
    slots[order as usize] = Some(0);
    let remaining = groups
        .iter()
        .position(|g| g.kind == GroupKind::RemainingPawns);
    if let Some(r) = remaining {
        slots[order2 as usize] = Some(r);
    }
    let mut others = (0..count).filter(|&g| g != 0 && Some(g) != remaining);
    for slot in slots.iter_mut() {
        if slot.is_none() {
            *slot = others.next();
        }
    }
    let mut product = 1u64;
    for g in slots.into_iter().flatten() {
        groups[g].factor = product;
        product *= groups[g].size;
    }
    (groups, product)
}

/// Índice da posição: `squares` segue a sequência de peças da sub-tabela, já com as cores
/// ajustadas e (com peões) com o líder em `squares[0]`. As casas são normalizadas aqui.
pub fn index(squares: &mut [usize], groups: &[Group], has_pawns: bool) -> Option<u64> {
    let t = &*TABLES;
    let leader = &groups[0];
    if has_pawns {
        if file(squares[0]) >= 4 {
            for s in squares.iter_mut() {
                *s = mirror_horizontal(*s);
            }
        }
    } else {
        if file(squares[0]) >= 4 {
            for s in squares.iter_mut() {
                *s = mirror_horizontal(*s);
            }
        }
        if rank(squares[0]) >= 4 {
            for s in squares.iter_mut() {
                *s = mirror_vertical(*s);
            }
        }
        if let Some(&first) = squares[..leader.len]
            .iter()
            .find(|&&s| off_diagonal(s) != 0)
            && off_diagonal(first) > 0
        {
            for s in squares.iter_mut() {
                *s = mirror_diagonal(*s);
            }
        }
    }

    let mut idx = if has_pawns {
        let mut others: Vec<usize> = squares[1..leader.len].to_vec();
        others.sort_by_key(|&s| t.pawn_twist(s));
        let mut local = t.lead_start[leader.len][squares[0]];
        for (m, &s) in others.iter().enumerate() {
            local += t.binomial(i64::from(t.pawn_twist(s)), m + 1);
        }
        local * leader.factor
    } else if leader.len == 3 {
        t.leader_three(squares) * leader.factor
    } else {
        t.leader_kings(squares)? * leader.factor
    };

    for g in &groups[1..] {
        let mut members: Vec<usize> = squares[g.start..g.start + g.len].to_vec();
        members.sort_unstable();
        let mut local = 0;
        for (m, &s) in members.iter().enumerate() {
            let before = squares[..g.start].iter().filter(|&&p| p < s).count() as i64;
            let mut position = s as i64 - before;
            if g.kind == GroupKind::RemainingPawns {
                position -= 8;
            }
            local += t.binomial(position, m + 1);
        }
        idx += local * g.factor;
    }
    Some(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(name: &str) -> usize {
        let b = name.as_bytes();
        (b[1] - b'1') as usize * 8 + (b[0] - b'a') as usize
    }

    #[test]
    fn triangle_lower_and_diagonal_numbering() {
        let t = &*TABLES;
        let order = ["b1", "c1", "d1", "c2", "d2", "d3", "a1", "b2", "c3", "d4"];
        for (i, name) in order.iter().enumerate() {
            assert_eq!(t.tri[sq(name)], Some(i as u8), "{name}");
        }
        assert_eq!(t.lower[sq("b1")], Some(0));
        assert_eq!(t.lower[sq("h4")], Some(21));
        assert_eq!(t.lower[sq("h7")], Some(27));
        assert_eq!(t.lower[sq("a1")], None);
    }

    #[test]
    fn pawn_twist_and_leader_sizes() {
        let t = &*TABLES;
        for (name, value) in [("a2", 47), ("h2", 46), ("b2", 35), ("d7", 1), ("e7", 0)] {
            assert_eq!(t.pawn_twist(sq(name)), value, "{name}");
        }
        assert_eq!(t.lead_size[1], [6, 6, 6, 6]);
        assert_eq!(t.lead_size[2], [252, 180, 108, 36]);
        assert_eq!(t.lead_size[3], [5201, 2645, 953, 125]);
        assert_eq!(t.lead_size[4], [70315, 25375, 5491, 295]);
    }

    #[test]
    fn two_kings_table() {
        let t = &*TABLES;
        let total = t.kk.iter().flatten().filter(|v| v.is_some()).count();
        assert_eq!(total as u64, LEADER_KINGS);
        // Rei no b1 (TRI = 0): a1, b1, c1, a2, b2, c2 são inválidas e d1 é o código 0.
        for name in ["a1", "b1", "c1", "a2", "b2", "c2"] {
            assert_eq!(t.kk[0][sq(name)], None, "{name}");
        }
        assert_eq!(t.kk[0][sq("d1")], Some(0));
    }

    #[test]
    fn sizes_of_known_tables() {
        // Códigos: 1 peão … 6 rei; +8 = preto da tabela.
        let (_, krvkn) = groups(&[14, 4, 6, 10], 1, 15, false, 0);
        assert_eq!(krvkn, 1_911_252);
        let (_, krrvk) = groups(&[6, 14, 4, 4], 0, 15, false, 0);
        assert_eq!(krrvk, 873_642);
        let (g, kpvk) = groups(&[1, 6, 14], 1, 15, true, 0);
        assert_eq!(kpvk, 23_436);
        assert_eq!((g[0].factor, g[1].factor, g[2].factor), (63, 1, 378));
        let (g, kpvkp) = groups(&[1, 9, 6, 14], 3, 2, true, 0);
        assert_eq!(kpvkp, 1_066_524);
        let factors: Vec<u64> = g.iter().map(|g| g.factor).collect();
        assert_eq!(factors, [177_754, 3782, 1, 62]);
    }

    #[test]
    fn worked_examples_of_the_spec() {
        // Exemplo A: KQvK lado 1 (rei preto, dama branca, rei branco), Rh8, Da1, Re1.
        let (g, _) = groups(&[14, 5, 6], 0, 15, false, 0);
        let mut p = [sq("h8"), sq("a1"), sq("e1")];
        assert_eq!(index(&mut p, &g, false), Some(30569));
        // Exemplo B: KPvK coluna d lado 0 (peão, rei branco, rei preto), Pe2, Re1, Rh1.
        let (g, _) = groups(&[1, 6, 14], 2, 15, true, 3);
        let mut p = [sq("e2"), sq("e1"), sq("h1")];
        assert_eq!(index(&mut p, &g, true), Some(3));
    }
}
