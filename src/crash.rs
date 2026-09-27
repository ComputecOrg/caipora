//! Relatório de queda: se a engine entrar em pânico, o motivo e os últimos `position` e `go`
//! recebidos vão para `caipora-crash.log`, ao lado do executável (além do stderr, que as
//! interfaces costumam descartar). Com isso uma queda rara vira uma posição reproduzível.

use std::io::Write;
use std::sync::Mutex;

/// Últimos comandos `position` e `go` recebidos.
static LAST_COMMANDS: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));

/// Guarda a linha se for um `position` ou um `go`; as demais não interessam para reproduzir.
pub fn record_command(line: &str) {
    let slot = match line.split_whitespace().next() {
        Some("position") => 0,
        Some("go") => 1,
        _ => return,
    };
    let mut last = LAST_COMMANDS.lock().unwrap_or_else(|e| e.into_inner());
    if slot == 0 {
        last.0 = line.to_string();
    } else {
        last.1 = line.to_string();
    }
}

/// Texto do relatório de uma queda.
pub fn report(panic_text: &str) -> String {
    let last = LAST_COMMANDS.lock().unwrap_or_else(|e| e.into_inner());
    format!(
        "caipora {} panicked: {panic_text}\nlast position: {}\nlast go: {}\n\n",
        env!("CARGO_PKG_VERSION"),
        last.0,
        last.1
    )
}

/// Instala o gancho: mantém a mensagem padrão no stderr e acrescenta o relatório ao arquivo.
pub fn install_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let path = std::env::current_exe()
            .map(|exe| exe.with_file_name("caipora-crash.log"))
            .unwrap_or_else(|_| "caipora-crash.log".into());
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = file.write_all(report(&info.to_string()).as_bytes());
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_carries_the_panic_and_the_last_position_and_go() {
        record_command("uci");
        record_command("position startpos moves e2e4");
        record_command("go wtime 1000 btime 1000");
        record_command("isready");
        record_command("position fen 8/8/8/8/8/8/8/K1k5 w - - 0 1");
        let report = report("index out of bounds");
        assert!(report.contains("index out of bounds"), "{report}");
        assert!(
            report.contains("position fen 8/8/8/8/8/8/8/K1k5 w - - 0 1"),
            "{report}"
        );
        assert!(report.contains("go wtime 1000 btime 1000"), "{report}");
        assert!(!report.contains("e2e4"), "{report}");
        assert!(!report.contains("isready"), "{report}");
    }
}
