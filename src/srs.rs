use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};

pub const EASE_MIN: f32 = 1.3;
pub const EASE_MAX: f32 = 2.9;
pub const EASE_START: f32 = 2.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Grade {
    pub fn from_input(s: &str) -> Option<Grade> {
        match s.trim() {
            "1" => Some(Grade::Again),
            "2" => Some(Grade::Hard),
            "3" => Some(Grade::Good),
            "4" => Some(Grade::Easy),
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

/// Aplica uma revisão (SM-2 simplificado, estilo Anki) e agenda a próxima.
pub fn apply(s: &mut SrsState, grade: Grade, today: NaiveDate) {
    let ease = match grade {
        Grade::Again => s.ease - 0.20,
        Grade::Hard => s.ease - 0.15,
        Grade::Good => s.ease,
        Grade::Easy => s.ease + 0.15,
    };
    let ease = round2(ease).clamp(EASE_MIN, EASE_MAX);

    let interval = match grade {
        Grade::Again => 1,
        Grade::Hard if s.reps == 0 => 1,
        Grade::Hard => ceil_days(s.interval as f32 * 1.2),
        Grade::Good if s.reps == 0 => 1,
        Grade::Good => ceil_days(s.interval as f32 * ease),
        Grade::Easy if s.reps == 0 => 3,
        Grade::Easy => ceil_days(s.interval as f32 * ease * 1.3),
    };

    if grade == Grade::Again {
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
    fn primeira_revisao_bom() {
        let mut s = new_state(today());
        apply(&mut s, Grade::Good, today());
        assert_eq!(s.interval, 1);
        assert_eq!(s.reps, 1);
        assert!((s.ease - 2.5).abs() < 1e-6);
        assert_eq!(s.due, NaiveDate::from_ymd_opt(2026, 11, 16).unwrap());
    }

    #[test]
    fn primeira_revisao_facil() {
        let mut s = new_state(today());
        apply(&mut s, Grade::Easy, today());
        assert_eq!(s.interval, 3);
        assert_eq!(s.due, NaiveDate::from_ymd_opt(2026, 11, 18).unwrap());
        assert!((s.ease - 2.65).abs() < 1e-6);
    }

    #[test]
    fn progressao_bom() {
        let mut s = new_state(today());
        apply(&mut s, Grade::Good, today()); // 1
        apply(&mut s, Grade::Good, today()); // 1 × 2.5 = 2.5 → 3
        apply(&mut s, Grade::Good, today()); // 3 × 2.5 = 7.5 → 8
        assert_eq!(s.interval, 8);
        assert_eq!(s.reps, 3);
    }

    #[test]
    fn errei_reseta_intervalo() {
        let mut s = state_with(2.5, 20, 5);
        apply(&mut s, Grade::Again, today());
        assert_eq!(s.interval, 1);
        assert_eq!(s.lapses, 1);
        assert_eq!(s.reps, 6);
        assert!((s.ease - 2.3).abs() < 1e-6);
        assert_eq!(s.due, NaiveDate::from_ymd_opt(2026, 11, 16).unwrap());
    }

    #[test]
    fn dificil_e_facil_ajustam() {
        let mut s = state_with(2.5, 10, 4);
        apply(&mut s, Grade::Hard, today());
        assert_eq!(s.interval, 12); // 10 × 1.2
        assert!((s.ease - 2.35).abs() < 1e-6);

        let mut s = state_with(2.5, 10, 4);
        apply(&mut s, Grade::Easy, today());
        assert_eq!(s.interval, 35); // 10 × 2.65 × 1.3 = 34.45 → 35
        assert!((s.ease - 2.65).abs() < 1e-6);
    }

    #[test]
    fn ease_dentro_dos_limites() {
        let mut s = state_with(1.35, 5, 3);
        apply(&mut s, Grade::Hard, today());
        assert!((s.ease - EASE_MIN).abs() < 1e-6);

        let mut s = state_with(2.85, 5, 3);
        apply(&mut s, Grade::Easy, today());
        assert!((s.ease - EASE_MAX).abs() < 1e-6);
    }

    #[test]
    fn entrada_valida() {
        assert_eq!(Grade::from_input("3"), Some(Grade::Good));
        assert_eq!(Grade::from_input(" 1 "), Some(Grade::Again));
        assert_eq!(Grade::from_input("5"), None);
        assert_eq!(Grade::from_input(""), None);
    }
}
