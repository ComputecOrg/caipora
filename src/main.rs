use std::fs::OpenOptions;
use std::io::{self, BufWriter};
use std::time::Instant;

use caipora::nnue::Network;
use caipora::{bench, datagen, uci};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        // `caipora bench [profundidade]`: usado pelo OpenBench e pelo CI para a assinatura da busca.
        Some("bench") => {
            let depth = args
                .get(1)
                .and_then(|d| d.parse().ok())
                .unwrap_or(bench::DEFAULT_DEPTH);
            println!("{}", uci::bench_line(&bench::run(depth)));
        }
        Some("datagen") => {
            if let Err(message) = run_datagen(&args[1..]) {
                eprintln!("datagen: {message}");
                eprintln!("uso: caipora datagen <partidas> <arquivo> [semente] [nós por lance]");
                std::process::exit(1);
            }
        }
        Some("validate") => {
            if let Err(message) = run_validate(&args[1..]) {
                eprintln!("validate: {message}");
                eprintln!("uso: caipora validate <rede|-> <posições.txt> [wdl]");
                std::process::exit(1);
            }
        }
        _ => uci::run(io::stdin().lock(), uci::Output::new(io::stdout())),
    }
}

/// `caipora datagen <partidas> <arquivo> [semente] [nós]`: acrescenta as posições ao arquivo e
/// mostra o progresso no stderr.
fn run_datagen(args: &[String]) -> Result<(), String> {
    let games = args
        .first()
        .and_then(|n| n.parse().ok())
        .ok_or("número de partidas inválido")?;
    let path = args.get(1).ok_or("falta o arquivo de saída")?;
    let number = |index: usize, default: u64| {
        args.get(index).map_or(Ok(default), |text| {
            text.parse().map_err(|_| format!("{text} não é número"))
        })
    };
    let config = datagen::Config {
        games,
        seed: number(2, 1)?,
        nodes: number(3, 5_000)?,
        random_plies: 8,
    };
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("{path}: {e}"))?;
    let mut out = BufWriter::new(file);
    let start = Instant::now();
    let progress = datagen::run(&config, &mut out, &mut |p| {
        if p.games % 100 == 0 {
            let rate = p.positions as f64 / start.elapsed().as_secs_f64().max(1e-9);
            eprintln!(
                "{} partidas, {} posições, {rate:.0} posições/s",
                p.games, p.positions
            );
        }
    })
    .map_err(|e| format!("{path}: {e}"))?;
    eprintln!(
        "fim: {} partidas, {} posições",
        progress.games, progress.positions
    );
    Ok(())
}

/// `caipora validate <rede|-> <posições.txt> [wdl]`: perda da rede (ou, com `-`, da avaliação à
/// mão) em posições no formato do datagen, a mesma do treino (wdl padrão 0,5).
fn run_validate(args: &[String]) -> Result<(), String> {
    let net_path = args.first().ok_or("falta a rede")?;
    let data_path = args.get(1).ok_or("falta o arquivo de posições")?;
    let wdl: f64 = match args.get(2) {
        Some(text) => text.parse().map_err(|_| format!("{text} não é número"))?,
        None => 0.5,
    };
    let text = std::fs::read_to_string(data_path).map_err(|e| format!("{data_path}: {e}"))?;
    // "-" no lugar da rede: a avaliação feita à mão, como linha de base.
    let result = if net_path == "-" {
        datagen::validation_loss(&caipora::eval::evaluate, &text, wdl)
    } else {
        let bytes = std::fs::read(net_path).map_err(|e| format!("{net_path}: {e}"))?;
        let net = Network::from_bytes(&bytes).map_err(|e| format!("{net_path}: {e}"))?;
        datagen::validation_loss(&|pos| net.evaluate(pos), &text, wdl)
    };
    let (loss, count) = result.ok_or("nenhuma posição válida")?;
    println!("validation loss {loss:.6} over {count} positions (wdl {wdl})");
    Ok(())
}
