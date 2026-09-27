use std::io;

use caipora::{bench, uci};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `caipora bench [profundidade]`: usado pelo OpenBench e pelo CI para a assinatura da busca.
    if args.first().map(String::as_str) == Some("bench") {
        let depth = args
            .get(1)
            .and_then(|d| d.parse().ok())
            .unwrap_or(bench::DEFAULT_DEPTH);
        println!("{}", uci::bench_line(&bench::run(depth)));
        return;
    }
    uci::run(io::stdin().lock(), uci::Output::new(io::stdout()));
}
