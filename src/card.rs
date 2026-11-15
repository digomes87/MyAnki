use serde::{Deserialize, Serialize};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// Um card de memorização. A fonte da verdade é `decks/*.jsonl`,
/// curado pelo agente — o binário só lê (e acrescenta via `anki add`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub front: String,
    pub back: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Vocabulário: gera também a direção verso→frente (`id#B`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub reverse: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Chaves de agendamento de um card: `id#A` e, se reverso, `id#B`.
pub fn keys_for(c: &Card) -> Vec<String> {
    let mut keys = vec![format!("{}#A", c.id)];
    if c.reverse {
        keys.push(format!("{}#B", c.id));
    }
    keys
}

/// Raiz do projeto: diretório que contém `decks/`.
/// Ordem: $ANKI_HOME → ancestrais do cwd → ancestrais do executável.
pub fn find_root() -> PathBuf {
    if let Ok(dir) = env::var("ANKI_HOME") {
        return PathBuf::from(dir);
    }
    let has_decks = |p: &Path| p.join("decks").is_dir();
    if let Ok(cwd) = env::current_dir() {
        if let Some(root) = cwd.ancestors().find(|p| has_decks(p)) {
            return root.to_path_buf();
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Some(root) = dir.ancestors().find(|p| has_decks(p)) {
                return root.to_path_buf();
            }
        }
    }
    PathBuf::from(".")
}

pub fn decks_dir(root: &Path) -> PathBuf {
    root.join("decks")
}

pub fn deck_names(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(decks_dir(root)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == OsStr::new("jsonl")) {
                if let Some(stem) = path.file_stem().and_then(OsStr::to_str) {
                    names.push(stem.to_owned());
                }
            }
        }
    }
    names.sort();
    names
}

pub fn load_deck(root: &Path, deck: &str) -> Result<Vec<Card>, String> {
    let path = decks_dir(root).join(format!("{deck}.jsonl"));
    let file = fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut cards = Vec::new();
    for (n, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| format!("{}: {e}", path.display()))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let card: Card = serde_json::from_str(trimmed)
            .map_err(|e| format!("{}:{}: {e}", path.display(), n + 1))?;
        cards.push(card);
    }
    Ok(cards)
}

/// Carrega os decks pedidos (todos, se `filter` for vazio), validando os nomes.
pub fn load_decks(root: &Path, filter: &[String]) -> Result<Vec<(String, Vec<Card>)>, String> {
    let available = deck_names(root);
    if available.is_empty() {
        return Err(format!(
            "nenhum deck em {} — o agente deve criar decks/*.jsonl primeiro",
            decks_dir(root).display()
        ));
    }
    let selected: Vec<String> = if filter.is_empty() {
        available.clone()
    } else {
        for name in filter {
            if !available.contains(name) {
                return Err(format!(
                    "deck desconhecido: {name}\ndecks disponíveis: {}",
                    available.join(", ")
                ));
            }
        }
        filter.to_vec()
    };
    selected
        .into_iter()
        .map(|name| Ok((name.clone(), load_deck(root, &name)?)))
        .collect()
}

/// Próximo ID sequencial do deck (`deck-0042`), escaneando os existentes.
pub fn next_id(deck: &str, cards: &[Card]) -> String {
    let max = cards
        .iter()
        .filter_map(|c| c.id.rsplit_once('-'))
        .filter(|(prefix, _)| *prefix == deck)
        .filter_map(|(_, suffix)| suffix.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{deck}-{:04}", max + 1)
}

pub fn sanitize_deck_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect()
}

pub fn append_card(root: &Path, deck: &str, card: &Card) -> io::Result<()> {
    let dir = decks_dir(root);
    fs::create_dir_all(&dir)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{deck}.jsonl")))?;
    writeln!(file, "{}", serde_json::to_string(card)?)
}
