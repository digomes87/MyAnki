/// Resultado da comparação entre resposta digitada e esperada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Exact,
    Close,
    Wrong,
    Blank,
}

/// Compara a resposta digitada com a esperada, tolerando
/// maiúsculas, acentos gráficos, pontuação e erros de digitação.
pub fn compare(expected: &str, given: &str) -> Match {
    let e = normalize(expected);
    let g = normalize(given);
    if g.is_empty() {
        return Match::Blank;
    }
    if e == g {
        return Match::Exact;
    }
    let tol = tolerance(e.chars().count());
    if levenshtein(&e, &g) <= tol {
        Match::Close
    } else {
        Match::Wrong
    }
}

fn tolerance(len: usize) -> usize {
    if len <= 5 {
        1
    } else if len <= 12 {
        2
    } else {
        3
    }
}

/// minúsculas · sem pontuação · espaços colapsados · ё→е
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for ch in s.chars() {
        let c = match ch {
            'ё' | 'Ё' => 'е',
            other => other,
        };
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            last_space = false;
        } else if c.is_whitespace() && !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1)
                .min(cur[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exato_ignorando_caixa_pontuacao() {
        assert_eq!(compare("wie geht's?", "WIE GEHTS"), Match::Exact);
        assert_eq!(compare("come stai?", "COME STAI"), Match::Exact);
    }

    #[test]
    fn quase_com_typo() {
        assert_eq!(compare("привет", "приветт"), Match::Close); // 1 letra a mais
        assert_eq!(compare("你好", "你号"), Match::Close); // 1 caractere trocado
        assert_eq!(compare("dog", "dogr"), Match::Close);
    }

    #[test]
    fn errado() {
        assert_eq!(compare("спасибо", "spasibo"), Match::Wrong); // alfabeto outro
        assert_eq!(compare("ciao", "arrivederci"), Match::Wrong);
    }

    #[test]
    fn vazio_e_branco() {
        assert_eq!(compare("ciao", ""), Match::Blank);
        assert_eq!(compare("ciao", "   "), Match::Blank);
    }

    #[test]
    fn io_normalizada() {
        assert_eq!(compare("ёлка", "елка"), Match::Exact);
    }

    #[test]
    fn levenshtein_classico() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("abc", "abc"), 0);
    }
}
