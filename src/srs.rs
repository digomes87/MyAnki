use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const EASE_MIN: f32 = 1.3;
pub const EASE_MAX: f32 = 2.9;
pub const EASE_START: f32 = 2.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    Wrong = 1,
    Correct = 2,
}

impl Grade {
    pub fn from_input(s: &str) -> Option<Grade> {
        match s.trim() {
            "1" => Some(Grade::Wrong),
            "2" => Some(Grade::Correct),
            _ => None,
        }
    }
}

/// Estado de agendamento de UM item de revisão (card ou uma direção de card reverso).
/// Vive em `state/schedule.json` — separado do conteúdo, que o agente pode editar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrsState {
    pub ease: f32,
    pub interval: u32,
    pub reps: u32,
    pub lapses: u32,
    pub due: NaiveDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<NaiveDate>,
}

pub fn new_state(today: NaiveDate) -> SrsState {
    SrsState {
        ease: EASE_START,
        interval: 0,
        reps: 0,
        lapses: 0,
        due: today,
        last: None,
    }
}

fn round2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

/// ceil tolerante a erro de ponto flutuante: 10 * 1.2 deve dar 12, não 13.
fn ceil_days(x: f32) -> u32 {
    ((x - 1e-4).ceil() as u32).max(1)
}

/// Aplica uma revisão (SM-2 simplificado, binário: acertei/errei) e agenda a próxima.
/// Erro desce o ease e reseta o intervalo; acerto progressão normal × ease.
pub fn apply(s: &mut SrsState, grade: Grade, today: NaiveDate) {
    let ease = match grade {
        Grade::Wrong => s.ease - 0.20,
        Grade::Correct => s.ease,
    };
    let ease = round2(ease).clamp(EASE_MIN, EASE_MAX);

    let interval = match grade {
        Grade::Wrong => 1,
        Grade::Correct if s.reps == 0 => 1,
        Grade::Correct => ceil_days(s.interval as f32 * ease),
    };

    if grade == Grade::Wrong {
        s.lapses += 1;
    }
    s.ease = ease;
    s.interval = interval;
    s.reps += 1;
    s.last = Some(today);
    s.due = today
        .checked_add_days(Days::new(interval as u64))
        .unwrap_or(today);
}

/// Irmãos ⇄ nunca dividem a sessão: ver a resposta de uma direção e em
/// seguida ser testado pela outra não mede memória — é o "bury siblings"
/// do Anki. Se A e B estão ambos vencidos, B (produção) adia 1 dia e A
/// segue na fila. Retorna quantas direções B foram adiadas.
pub fn space_siblings(schedule: &mut BTreeMap<String, SrsState>, today: NaiveDate) -> usize {
    let pairs: Vec<(String, String)> = schedule
        .keys()
        .filter_map(|b| b.strip_suffix("#B").map(|id| (format!("{id}#A"), b.to_owned())))
        .collect();
    let mut moved = 0usize;
    for (a_key, b_key) in pairs {
        let a_due = schedule.get(&a_key).is_some_and(|a| a.due <= today);
        let b_due = schedule.get(&b_key).is_some_and(|b| b.due <= today);
        if !a_due || !b_due {
            continue;
        }
        let Some(b) = schedule.get_mut(&b_key) else { continue };
        b.due = today.checked_add_days(Days::new(1)).unwrap_or(today);
        moved += 1;
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 15).unwrap()
    }

    fn state_with(ease: f32, interval: u32, reps: u32) -> SrsState {
        SrsState {
            ease,
            interval,
            reps,
            lapses: 0,
            due: today(),
            last: None,
        }
    }

    #[test]
    fn primeira_revisao_acertada() {
        let mut s = new_state(today());
        apply(&mut s, Grade::Correct, today());
        assert_eq!(s.interval, 1);
        assert_eq!(s.reps, 1);
        assert!((s.ease - 2.5).abs() < 1e-6);
        assert_eq!(s.due, NaiveDate::from_ymd_opt(2026, 11, 16).unwrap());
    }

    #[test]
    fn progressao_acertos() {
        let mut s = new_state(today());
        apply(&mut s, Grade::Correct, today()); // 1
        apply(&mut s, Grade::Correct, today()); // 1 × 2.5 = 2.5 → 3
        apply(&mut s, Grade::Correct, today()); // 3 × 2.5 = 7.5 → 8
        assert_eq!(s.interval, 8);
        assert_eq!(s.reps, 3);
    }

    #[test]
    fn errei_reseta_intervalo() {
        let mut s = state_with(2.5, 20, 5);
        apply(&mut s, Grade::Wrong, today());
        assert_eq!(s.interval, 1);
        assert_eq!(s.lapses, 1);
        assert_eq!(s.reps, 6);
        assert!((s.ease - 2.3).abs() < 1e-6);
        assert_eq!(s.due, NaiveDate::from_ymd_opt(2026, 11, 16).unwrap());
    }

    #[test]
    fn erro_encurta_proxima_progressao() {
        // após 1 erro (ease 2.3), a progressão de acertos fica mais lenta
        let mut s = state_with(2.5, 10, 4);
        apply(&mut s, Grade::Wrong, today());
        apply(&mut s, Grade::Correct, today()); // 1 × 2.3 = 2.3 → 3
        assert_eq!(s.interval, 3);
        assert!((s.ease - 2.3).abs() < 1e-6);
    }

    #[test]
    fn ease_dentro_dos_limites() {
        let mut s = state_with(1.35, 5, 3);
        apply(&mut s, Grade::Wrong, today());
        assert!((s.ease - EASE_MIN).abs() < 1e-6);
    }

    fn sched_with(due: NaiveDate) -> SrsState {
        SrsState {
            ease: 2.5,
            interval: 3,
            reps: 2,
            lapses: 0,
            due,
            last: None,
        }
    }

    #[test]
    fn irmaos_vencidos_juntos_adia_b() {
        // lockstep: A e B vencem hoje → B escorrega, A fica
        let mut s = std::collections::BTreeMap::new();
        s.insert("it-1#A".to_owned(), sched_with(today()));
        s.insert("it-1#B".to_owned(), sched_with(today()));
        assert_eq!(space_siblings(&mut s, today()), 1);
        assert_eq!(s["it-1#A"].due, today());
        assert_eq!(
            s["it-1#B"].due,
            NaiveDate::from_ymd_opt(2026, 11, 16).unwrap()
        );
    }

    #[test]
    fn irmaos_atrasados_com_dues_diferentes_tambem_separam() {
        // pular dias faz os dois caírem na mesma fila, mesmo com dues distintos
        let mut s = std::collections::BTreeMap::new();
        s.insert(
            "it-1#A".to_owned(),
            sched_with(NaiveDate::from_ymd_opt(2026, 11, 13).unwrap()),
        );
        s.insert(
            "it-1#B".to_owned(),
            sched_with(NaiveDate::from_ymd_opt(2026, 11, 14).unwrap()),
        );
        assert_eq!(space_siblings(&mut s, today()), 1);
        assert_eq!(s["it-1#A"].due, NaiveDate::from_ymd_opt(2026, 11, 13).unwrap());
        assert_eq!(
            s["it-1#B"].due,
            NaiveDate::from_ymd_opt(2026, 11, 16).unwrap()
        );
    }

    #[test]
    fn so_b_adia_e_so_quando_ambos_vencem() {
        let tomorrow = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        // B vence depois de A: nada a fazer
        let mut s = std::collections::BTreeMap::new();
        s.insert("it-1#A".to_owned(), sched_with(today()));
        s.insert("it-1#B".to_owned(), sched_with(tomorrow));
        assert_eq!(space_siblings(&mut s, today()), 0);
        assert_eq!(s["it-1#B"].due, tomorrow);

        // ambos no futuro (nem vencidos): o adiantamento é preguiçoso,
        // a checagem roda de novo na sessão do dia em que vencerem
        let mut s = std::collections::BTreeMap::new();
        s.insert("it-1#A".to_owned(), sched_with(tomorrow));
        s.insert("it-1#B".to_owned(), sched_with(tomorrow));
        assert_eq!(space_siblings(&mut s, today()), 0);
    }

    #[test]
    fn b_sem_a_nao_existe() {
        let mut s = std::collections::BTreeMap::new();
        s.insert("it-1#B".to_owned(), sched_with(today()));
        assert_eq!(space_siblings(&mut s, today()), 0);
    }

    #[test]
    fn entrada_valida() {
        assert_eq!(Grade::from_input("2"), Some(Grade::Correct));
        assert_eq!(Grade::from_input(" 1 "), Some(Grade::Wrong));
        assert_eq!(Grade::from_input("3"), None); // não existe mais 3–4
        assert_eq!(Grade::from_input(""), None);
    }
}
