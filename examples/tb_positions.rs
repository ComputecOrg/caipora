//! Sorteia posições legais com o material pedido, para montar o oráculo das tablebases.
//! Uso: `cargo run --release --example tb_positions -- <semente> <por material> KQvK KRvKP ...`

use caipora::movegen::generate_legal;
use caipora::position::Position;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut seed: u64 = args[0].parse().unwrap();
    let per: usize = args[1].parse().unwrap();
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for material in &args[2..] {
        let (white, black) = material.split_once('v').unwrap();
        let mut found = 0;
        while found < per {
            let mut board = [None; 64];
            let mut ok = true;
            for (pieces, upper) in [(white, true), (black, false)] {
                for p in pieces.chars() {
                    let sq = loop {
                        let s = (rand() % 64) as usize;
                        if board[s].is_none() {
                            break s;
                        }
                    };
                    if p == 'P' && (sq / 8 == 0 || sq / 8 == 7) {
                        ok = false;
                    }
                    board[sq] = Some(if upper { p } else { p.to_ascii_lowercase() });
                }
            }
            if !ok {
                continue;
            }
            let mut rows = Vec::new();
            for r in (0..8).rev() {
                let (mut row, mut empty) = (String::new(), 0);
                for f in 0..8 {
                    match board[r * 8 + f] {
                        Some(c) => {
                            if empty > 0 {
                                row.push_str(&empty.to_string());
                                empty = 0;
                            }
                            row.push(c);
                        }
                        None => empty += 1,
                    }
                }
                if empty > 0 {
                    row.push_str(&empty.to_string());
                }
                rows.push(row);
            }
            let stm = if rand() % 2 == 0 { "w" } else { "b" };
            let fen = format!("{} {stm} - - 0 1", rows.join("/"));
            let Ok(pos) = Position::from_fen(&fen) else {
                continue;
            };
            // Ilegal se quem não joga está em xeque; posição terminal não interessa.
            let other = pos.side_to_move().flip();
            if pos.is_attacked(pos.king_square(other), pos.side_to_move())
                || generate_legal(&pos).is_empty()
            {
                continue;
            }
            println!("{fen}");
            found += 1;
        }
    }
}
