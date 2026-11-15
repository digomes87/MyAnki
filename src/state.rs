use crate::srs::SrsState;
use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Progresso do usuário — `state/schedule.json` + `state/history.jsonl`.
/// Esta área nunca é editada pelo agente.
pub struct Store {
    root: PathBuf,
    pub schedule: BTreeMap<String, SrsState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub date: NaiveDate,
    pub deck: String,
    pub id: String,
    pub grade: u8,
}

impl Store {
    pub fn load(root: &Path) -> Store {
        let schedule = fs::read_to_string(root.join("state/schedule.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Store {
            root: root.to_path_buf(),
            schedule,
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let dir = self.root.join("state");
        fs::create_dir_all(&dir)?;
        let mut json = serde_json::to_string_pretty(&self.schedule)?;
        json.push('\n');
        fs::write(dir.join("schedule.json"), json)
    }

    pub fn append_history(&self, entry: &HistoryEntry) -> io::Result<()> {
        let dir = self.root.join("state");
        fs::create_dir_all(&dir)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("history.jsonl"))?;
        writeln!(file, "{}", serde_json::to_string(entry)?)
    }

    pub fn history(&self) -> Vec<HistoryEntry> {
        fs::read_to_string(self.root.join("state/history.jsonl"))
            .ok()
            .map(|s| {
                s.lines()
                    .filter(|l| !l.trim().is_empty())
                    .filter_map(|l| serde_json::from_str(l).ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Streak: dias consecutivos com revisão, terminando hoje (0 se hoje não revisou).
pub fn streak(history: &[HistoryEntry], today: NaiveDate) -> u32 {
    let days: HashSet<NaiveDate> = history.iter().map(|e| e.date).collect();
    let mut count = 0;
    let mut day = today;
    while days.contains(&day) {
        count += 1;
        match day.pred_opt() {
            Some(prev) => day = prev,
            None => break,
        }
    }
    count
}

/// Retenção (proporção de acertos) numa janela de dias até hoje.
pub fn retention(history: &[HistoryEntry], window_days: u32, today: NaiveDate) -> Option<f32> {
    let cutoff = today.checked_sub_days(Days::new(window_days as u64))?;
    let recent: Vec<&HistoryEntry> = history.iter().filter(|e| e.date >= cutoff).collect();
    if recent.is_empty() {
        return None;
    }
    let hits = recent.iter().filter(|e| e.grade > 1).count();
    Some(hits as f32 / recent.len() as f32)
}
