use std::io::{self, IsTerminal, Write};
use std::sync::OnceLock;

static COLOR: OnceLock<bool> = OnceLock::new();

fn color() -> bool {
    *COLOR.get_or_init(|| io::stdout().is_terminal())
}

fn wrap(code: &str, s: &str) -> String {
    if color() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn bold(s: &str) -> String {
    wrap("1", s)
}
pub fn dim(s: &str) -> String {
    wrap("2", s)
}
pub fn red(s: &str) -> String {
    wrap("31", s)
}
pub fn green(s: &str) -> String {
    wrap("32", s)
}
pub fn yellow(s: &str) -> String {
    wrap("33", s)
}
pub fn cyan(s: &str) -> String {
    wrap("36", s)
}

/// Imprime sem quebra de linha e força flush (necessário antes de ler stdin).
pub fn out(s: &str) {
    print!("{s}");
    let _ = io::stdout().flush();
}

/// Lê uma linha do stdin. `None` = EOF (Ctrl-D / pipe fechado).
pub fn read_line() -> io::Result<Option<String>> {
    let mut line = String::new();
    let n = io::stdin().read_line(&mut line)?;
    if n == 0 {
        Ok(None)
    } else {
        Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
    }
}
