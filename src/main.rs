mod answer;
mod card;
mod srs;
mod state;
mod ui;

use card::Card;
use chrono::Local;
use clap::{Parser, Subcommand};
use srs::Grade;
use state::{HistoryEntry, Store};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "anki",
    version,
    about = "Repetição espaçada no terminal — cards curados pelo agente, revisão por sua conta"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Sessão de revisão do dia
    Review {
        /// Decks a revisar (padrão: todos)
        decks: Vec<String>,
        /// Máximo de cards novos por dia
        #[arg(long, default_value_t = 10)]
        new: u32,
    },
    /// Adiciona um card a um deck (interativo)
    Add { deck: String },
    /// Mostra vencidos e novos por deck
    Due,
    /// Estatísticas de progresso
    Stats,
    /// Estudo livre: embaralha e repete à vontade, sem alterar o agendamento
    Study {
        /// Decks (padrão: todos)
        decks: Vec<String>,
        /// Foca nos itens com erro no histórico (lapses ≥ 1)
        #[arg(long)]
        errei: bool,
    },
    /// Lista os cards de um deck (ou de todos)
    List { deck: Option<String> },
    /// Ranking dos cards mais errados no histórico — guia para quebrar os difíceis
    Erros,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let root = card::find_root();
    let result = match cli.command {
        Some(Command::Review { decks, new }) => run_review(&root, &decks, new),
        Some(Command::Add { deck }) => run_add(&root, &deck),
        Some(Command::Due) => run_overview(&root, true),
        Some(Command::Stats) => run_stats(&root),
        Some(Command::Study { decks, errei }) => run_study(&root, &decks, errei),
        Some(Command::List { deck }) => run_list(&root, deck.as_deref()),
        Some(Command::Erros) => run_errors(&root),
        None => run_panel(&root),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("{} {msg}", ui::red("erro:"));
            ExitCode::FAILURE
        }
    }
}

/// Um item agendável: um card, ou uma das duas direções de um card reverso.
#[derive(Clone)]
struct Item {
    key: String,
    deck: String,
    card: Card,
    flipped: bool,
}

impl Item {
    /// Cards marcados como `typed` pedem resposta digitada.
    /// Em cards reversos, só a direção B (produção: pt → língua);
    /// a direção A (reconhecimento) tem sinônimos demais e fica auto-avaliada.
    fn wants_typing(&self) -> bool {
        self.card.typed && (!self.card.reverse || self.flipped)
    }

    fn front(&self) -> &str {
        if self.flipped {
            &self.card.back
        } else {
            &self.card.front
        }
    }
    fn back(&self) -> &str {
        if self.flipped {
            &self.card.front
        } else {
            &self.card.back
        }
    }
}

/// Resultado de um card apresentado: nota dada, ou pulo (não registra nada).
enum Outcome {
    Graded(Grade),
    Skipped,
}

/// Mostra a frente do item, coleta a resposta (digitada ou revelada)
/// e lê a avaliação Errei/Acertei. `allow_quit`: no modo livre, "q" encerra a sessão.
/// "s" em qualquer prompt pula o card. Retorna `None` em EOF/quit.
fn present_item(it: &Item, allow_quit: bool) -> Option<Outcome> {
    // cards `typed`: o prompt é só a 1ª linha do lado visível;
    // as linhas restantes viram referência mostrada depois da resposta
    let shown_lines: Vec<&str> = it.front().lines().collect();
    let (prompt, reference): (&str, &[&str]) = if it.wants_typing() && shown_lines.len() > 1 {
        (shown_lines[0], &shown_lines[1..])
    } else {
        (it.front(), &[])
    };
    println!("{}", ui::bold(prompt));

    let can_type = it.wants_typing() && it.back().lines().count() == 1;
    if can_type {
        ui::out(&format!("\n{} ", ui::cyan("Resposta:")));
        match ui::read_line() {
            Ok(Some(input)) => {
                if allow_quit && input.trim().eq_ignore_ascii_case("q") {
                    return None;
                }
                if input.trim().eq_ignore_ascii_case("s") {
                    return Some(Outcome::Skipped);
                }
                match answer::compare(it.back(), &input) {
                    answer::Match::Exact => {
                        println!("\n  {} {}", ui::green("✓ exato:"), it.back())
                    }
                    answer::Match::Close => println!(
                        "\n  {} {} · {} {}",
                        ui::yellow("≈ quase (typo?)"),
                        input,
                        ui::dim("esperado:"),
                        it.back()
                    ),
                    answer::Match::Wrong => println!(
                        "\n  {} {} · {} {}",
                        ui::red("✗ você digitou:"),
                        input,
                        ui::dim("esperado:"),
                        it.back()
                    ),
                    answer::Match::Blank => {
                        println!("\n  {} {}", ui::red("✗ sem resposta · esperado:"), it.back())
                    }
                }
            }
            _ => return None,
        }
        for line in reference {
            println!("  {} {}", ui::dim("ℹ"), line);
        }
    } else {
        let hint = if allow_quit { "(Enter · q sai)" } else { "(Enter)" };
        ui::out(&format!("\n{} ", ui::dim(&format!("⏎ revelar {hint}"))));
        match ui::read_line() {
            Ok(Some(input)) => {
                if allow_quit && input.trim().eq_ignore_ascii_case("q") {
                    return None;
                }
                if input.trim().eq_ignore_ascii_case("s") {
                    return Some(Outcome::Skipped);
                }
            }
            _ => return None,
        }
        println!();
        for line in it.back().lines() {
            println!("  {} {}", ui::green("│"), line);
        }
    }
    read_grade().map(Outcome::Graded)
}

fn build_items(decks: &[(String, Vec<Card>)]) -> Vec<Item> {
    let mut items = Vec::new();
    for (deck, cards) in decks {
        for c in cards {
            items.push(Item {
                key: format!("{}#A", c.id),
                deck: deck.clone(),
                card: c.clone(),
                flipped: false,
            });
            if c.reverse {
                items.push(Item {
                    key: format!("{}#B", c.id),
                    deck: deck.clone(),
                    card: c.clone(),
                    flipped: true,
                });
            }
        }
    }
    items
}

fn run_review(root: &Path, deck_filter: &[String], new_per_day: u32) -> Result<(), String> {
    let decks = card::load_decks(root, deck_filter)?;
    let items = build_items(&decks);
    let mut store = Store::load(root);
    let today = Local::now().date_naive();

    // ⇄ irmãos não dividem a sessão: com a resposta de uma direção ainda na
    // tela, a outra deixa de medir memória. Pares ambos vencidos → B adia 1 dia.
    let spaced = srs::space_siblings(&mut store.schedule, today);
    if spaced > 0 {
        store.save().map_err(|e| format!("salvando state/schedule.json: {e}"))?;
        println!(
            "{}",
            ui::dim(&format!(
                "→ {spaced} direções ⇄ adiadas para amanhã (irmãs de vencidos de hoje)"
            ))
        );
        println!();
    }

    // vencidos: têm estado e a data de revisão chegou
    let mut due: Vec<&Item> = items
        .iter()
        .filter(|it| {
            store
                .schedule
                .get(&it.key)
                .is_some_and(|s| s.due <= today)
        })
        .collect();
    due.sort_by(|a, b| {
        let da = store.schedule[&a.key].due;
        let db = store.schedule[&b.key].due;
        da.cmp(&db).then_with(|| a.key.cmp(&b.key))
    });

    // novos: sem estado; respeitando o limite diário (estreias de hoje vêm do histórico)
    let selected_keys: HashSet<String> = items.iter().map(|it| it.key.clone()).collect();
    let introduced_today = state::introduced_today(&store.history(), today, &selected_keys);
    let new_slots = new_per_day.saturating_sub(introduced_today) as usize;
    let fresh: Vec<&Item> = items
        .iter()
        .filter(|it| {
            // estreia de B espera A: só entra quando A já foi vista E não vence
            // hoje — as duas direções de um card nunca dividem a sessão
            !it.flipped
                || store
                    .schedule
                    .get(&format!("{}#A", it.card.id))
                    .is_some_and(|s| s.due > today)
        })
        .filter(|it| !store.schedule.contains_key(&it.key))
        .take(new_slots)
        .collect();

    let due_count = due.len();
    let fresh_count = fresh.len();
    let remaining_new = items
        .iter()
        .filter(|it| !store.schedule.contains_key(&it.key))
        .count() as u32;
    let mut queue: Vec<Item> = due.into_iter().chain(fresh).cloned().collect();
    if queue.is_empty() {
        if remaining_new > 0 && introduced_today >= new_per_day {
            println!("{}", ui::yellow("Fila vazia: limite diário de novos já estourado"));
            println!(
                "  hoje você já estreou {} novos neste(s) deck(s) — o comando pediu teto {new_per_day}",
                introduced_today
            );
            println!("  restam {remaining_new} novos por estrear");
            println!(
                "  {} ou volte amanhã (os de hoje voltam como vencidos)",
                ui::cyan(&format!("→ use --new {} para estrear mais agora", introduced_today + 1))
            );
            println!(
                "  {}",
                ui::dim("→ `anki study` para reforço livre sem estrear novos")
            );
        } else {
            println!(
                "{}",
                ui::green("Nada pra revisar hoje ✓ todos os cards estão agendados — volte amanhã")
            );
            println!(
                "{}",
                ui::dim("→ `anki study` para repetir à vontade sem mexer no agendamento")
            );
        }
        return Ok(());
    }

    let deck_list = decks
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    println!("{}", ui::bold(&format!("━━━ anki · {deck_list} ━━━")));
    println!(
        "{} · {}",
        ui::cyan(&format!("{due_count} vencidos")),
        ui::cyan(&format!("{fresh_count} novos"))
    );
    println!("{}\n", ui::dim("s pula o card (volta no fim, sem registrar nada)"));

    let start = Instant::now();
    let mut answered = 0usize;
    let mut errors = 0usize;
    let mut requeued: HashSet<String> = HashSet::new();
    let mut interrupted = false;

    while !queue.is_empty() {
        let total = answered + queue.len();
        let it = queue.remove(0);

        let tags = if it.card.tags.is_empty() {
            String::new()
        } else {
            format!(" {}", ui::dim(&format!("· {}", it.card.tags.join(", "))))
        };
        let direction = if it.flipped {
            format!(" {}", ui::dim("⇄ inverso"))
        } else {
            String::new()
        };
        println!(
            "{} {}{}{}",
            ui::dim(&format!("[{}/{}]", answered + 1, total)),
            ui::cyan(&it.deck),
            direction,
            tags
        );
        let Some(outcome) = present_item(&it, false) else {
            interrupted = true;
            break;
        };
        let grade = match outcome {
            Outcome::Skipped => {
                println!("  {}", ui::dim("↷ pulado — volta no fim da sessão, sem registrar"));
                queue.push(it);
                println!();
                continue;
            }
            Outcome::Graded(g) => g,
        };

        // aplica o agendamento e salva NA HORA: Ctrl-C não perde progresso
        let entry = store
            .schedule
            .entry(it.key.clone())
            .or_insert_with(|| srs::new_state(today));
        srs::apply(entry, grade, today);
        let interval = entry.interval;
        let due_date = entry.due;
        store
            .save()
            .map_err(|e| format!("salvando state/schedule.json: {e}"))?;
        store
            .append_history(&HistoryEntry {
                date: today,
                deck: it.deck.clone(),
                id: it.key.clone(),
                grade: grade as u8,
            })
            .map_err(|e| format!("salvando state/history.jsonl: {e}"))?;

        answered += 1;
        if grade == Grade::Wrong {
            errors += 1;
            println!(
                "  {} {}",
                ui::red("✗ Errei"),
                ui::dim("volta no fim da sessão · próxima em 1d")
            );
            if requeued.insert(it.key.clone()) {
                queue.push(it);
            }
        } else {
            println!(
                "  {} {}",
                ui::green(format!("✓ {}", grade_label(grade)).as_str()),
                ui::dim(&format!("próxima em {interval}d ({})", due_date.format("%d/%m")))
            );
        }
        println!();
    }

    if interrupted {
        println!("{}", ui::yellow("— sessão interrompida, progresso salvo —"));
    }
    let acc = if answered > 0 {
        (answered - errors) * 100 / answered
    } else {
        0
    };
    let hist = store.history();
    let streak_days = state::streak(&hist, today);
    println!("{}", ui::bold("━━━ sessão concluída ━━━"));
    println!(
        "{answered} cards · {errors} erros · {acc}% de acerto · {}s · streak {} {}",
        start.elapsed().as_secs(),
        streak_days,
        if streak_days == 1 { "dia" } else { "dias" }
    );
    Ok(())
}

/// Pergunta sim/não no terminal (vazio = não).
fn ask_yes(prompt: &str) -> Result<bool, String> {
    ui::out(&format!("{} ", ui::cyan(prompt)));
    Ok(matches!(
        ui::read_line()
            .map_err(|e| e.to_string())?
            .unwrap_or_default()
            .trim(),
        "s" | "S" | "sim" | "Sim"
    ))
}

fn grade_label(g: Grade) -> &'static str {    match g {
        Grade::Wrong => "Errei",
        Grade::Correct => "Acertei",
    }
}

/// Lê a avaliação binária `1 Errei / 2 Acertei` (Enter = Acertei). `None` em EOF.
fn read_grade() -> Option<Grade> {
    loop {
        ui::out(&format!(
            "\n  {}  {}  {} ",
            ui::red("1 ✗ Errei"),
            ui::green("2 ✓ Acertei"),
            ui::dim("(Enter = Acertei)")
        ));
        match ui::read_line() {
            Ok(Some(s)) if s.trim().is_empty() => return Some(Grade::Correct),
            Ok(Some(s)) => match Grade::from_input(&s) {
                Some(g) => return Some(g),
                None => println!("{}", ui::dim("  digite 1 ou 2 (ou Enter = Acertei)")),
            },
            _ => return None,
        }
    }
}

/// Embaralha a ordem de estudo, depois afasta direções irmãs que caíram
/// adjacentes (a resposta de uma estaria na tela para a outra). Pares no
/// fim da lista sem vizinho pra trocar ficam como estão — decks mínimos
/// não têm pra onde fugir.
fn shuffle_apart(order: &mut Vec<usize>, items: &[Item], rng: &mut impl rand::Rng) {
    use rand::seq::SliceRandom;
    order.shuffle(rng);
    for i in 0..order.len().saturating_sub(1) {
        if items[order[i]].card.id == items[order[i + 1]].card.id && i + 2 < order.len() {
            order.swap(i + 1, i + 2);
        }
    }
}

/// Modo livre: embaralha e repete à vontade — NADA é agendado ou registrado.
/// É o reforço extra por conta do usuário; o ritual de agendamento continua
/// sendo o `review`.
fn run_study(root: &Path, deck_filter: &[String], errei: bool) -> Result<(), String> {
    let decks = card::load_decks(root, deck_filter)?;
    let items = build_items(&decks);
    // `--errei`: reforço direcionado — só itens com erro no histórico.
    // O schedule é só lido; study nunca agenda nem registra nada.
    let items = if errei {
        let store = Store::load(root);
        let weak: Vec<Item> = items
            .into_iter()
            .filter(|it| store.schedule.get(&it.key).is_some_and(|s| s.lapses > 0))
            .collect();
        if weak.is_empty() {
            println!(
                "{}",
                ui::green("Nada com erro no histórico ✓ os lapses moram em state/schedule.json")
            );
            return Ok(());
        }
        println!(
            "{}\n",
            ui::yellow(&format!("foco: {} itens com erro no histórico", weak.len()))
        );
        weak
    } else {
        items
    };
    if items.is_empty() {
        return Err("deck(s) sem cards".into());
    }
    let mut rng = rand::rng();
    let mut order: Vec<usize> = (0..items.len()).collect();
    shuffle_apart(&mut order, &items, &mut rng);
    let mut pos = 0usize;
    let mut count = 0usize;
    let mut errors = 0usize;
    let start = Instant::now();
    let deck_list = decks
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    println!(
        "{}",
        ui::bold(&format!("━━━ anki · estudo livre · {deck_list} ━━━"))
    );
    println!(
        "{}\n",
        ui::dim("repetição sem agenda: não mexe em vencimentos nem histórico — s pula · q sai")
    );

    loop {
        if pos == order.len() {
            shuffle_apart(&mut order, &items, &mut rng); // nova passada, nova ordem
            pos = 0;
        }
        let it = &items[order[pos]];
        pos += 1;

        let tags = if it.card.tags.is_empty() {
            String::new()
        } else {
            format!(" {}", ui::dim(&format!("· {}", it.card.tags.join(", "))))
        };
        let direction = if it.flipped {
            format!(" {}", ui::dim("⇄ inverso"))
        } else {
            String::new()
        };
        println!(
            "{} {}{}{}",
            ui::dim(&format!("〔·〕")),
            ui::cyan(&it.deck),
            direction,
            tags
        );

        let Some(outcome) = present_item(it, true) else {
            break;
        };
        let grade = match outcome {
            Outcome::Skipped => {
                println!("  {}\n", ui::dim("↷ pulado"));
                continue;
            }
            Outcome::Graded(g) => g,
        };
        count += 1;
        if grade == Grade::Wrong {
            errors += 1;
            println!("  {}\n", ui::red("✗ anote o erro e siga"));
        } else {
            println!("  {}\n", ui::green(format!("✓ {}", grade_label(grade)).as_str()));
        }
    }

    let acc = if count > 0 {
        (count - errors) * 100 / count
    } else {
        0
    };
    println!("{}", ui::bold("━━━ estudo livre concluído ━━━"));
    println!(
        "{count} revisões · {errors} erros · {acc}% de acerto · {}s",
        start.elapsed().as_secs()
    );
    Ok(())
}

fn run_add(root: &Path, deck_name: &str) -> Result<(), String> {
    let deck = card::sanitize_deck_name(deck_name);
    let is_new = !card::decks_dir(root).join(format!("{deck}.jsonl")).exists();
    let existing = card::load_deck(root, &deck).unwrap_or_default();

    println!("{}", ui::bold(&format!("novo card → {deck}")));
    ui::out(&format!("{} ", ui::cyan("Frente:")));
    let front = ui::read_line()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if front.trim().is_empty() {
        return Err("frente vazia — nada adicionado".into());
    }

    ui::out(&format!(
        "{} {}\n",
        ui::cyan("Verso:"),
        ui::dim("(termine com uma linha vazia)")
    ));
    let mut back_lines = Vec::new();
    loop {
        ui::out("  ");
        match ui::read_line().map_err(|e| e.to_string())? {
            Some(line) if line.trim().is_empty() => break,
            Some(line) => back_lines.push(line),
            None => break,
        }
    }
    let back = back_lines.join("\n");
    if back.trim().is_empty() {
        return Err("verso vazio — nada adicionado".into());
    }

    ui::out(&format!("{} ", ui::cyan("Tags (vírgula):")));
    let tags: Vec<String> = ui::read_line()
        .map_err(|e| e.to_string())?
        .unwrap_or_default()
        .split(',')
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect();

    let reverse = ask_yes("Reverso? treina as duas direções [s/N]:")?;
    let typed = ask_yes("Digitável? resposta curta e exata — você digita e eu comparo [s/N]:")?;

    let id = card::next_id(&deck, &existing);
    let new_card = Card {
        id: id.clone(),
        front,
        back,
        tags,
        reverse,
        typed,
        note: None,
    };
    card::append_card(root, &deck, &new_card).map_err(|e| e.to_string())?;
    let extra = if is_new {
        ui::dim(" (novo deck criado)")
    } else {
        String::new()
    };
    println!("{} {}{}", ui::green("✓ adicionado"), ui::bold(&id), extra);
    Ok(())
}

/// Tabela vencidos/novos por deck — compartilhada entre `due` e `stats`.
fn run_overview(root: &Path, with_hint: bool) -> Result<(), String> {
    let today = Local::now().date_naive();
    let store = Store::load(root);
    let names = card::deck_names(root);
    if names.is_empty() {
        println!(
            "{}",
            ui::dim(&format!(
                "nenhum deck em {} — o agente deve criar decks/*.jsonl",
                card::decks_dir(root).display()
            ))
        );
        return Ok(());
    }
    println!(
        "{:<12} {:>6} {:>9} {:>6}",
        "deck", "cards", "vencidos", "novos"
    );
    let (mut t_cards, mut t_due, mut t_new) = (0usize, 0usize, 0usize);
    for name in &names {
        let cards = card::load_deck(root, name)?;
        let keys: Vec<String> = cards.iter().flat_map(card::keys_for).collect();
        let d = keys
            .iter()
            .filter(|k| store.schedule.get(k.as_str()).is_some_and(|s| s.due <= today))
            .count();
        let n = keys
            .iter()
            .filter(|k| !store.schedule.contains_key(k.as_str()))
            .count();
        t_cards += keys.len();
        t_due += d;
        t_new += n;
        println!("{:<12} {:>6} {:>9} {:>6}", name, keys.len(), d, n);
    }
    println!();
    if with_hint && t_due + t_new > 0 {
        println!("{}", ui::dim("→ rode `anki` para abrir o painel e revisar"));
    } else {
        println!(
            "{}",
            ui::dim(&format!(
                "total: {t_cards} cards · {t_due} vencidos · {t_new} novos"
            ))
        );
    }
    Ok(())
}

fn run_stats(root: &Path) -> Result<(), String> {
    run_overview(root, false)?;
    let store = Store::load(root);
    let hist = store.history();
    let today = Local::now().date_naive();
    if hist.is_empty() {
        println!("{}", ui::dim("sem histórico ainda — faça sua primeira sessão"));
        return Ok(());
    }
    let today_count = hist.iter().filter(|e| e.date == today).count();
    let streak_days = state::streak(&hist, today);
    let ret = state::retention(&hist, 30, today)
        .map(|r| format!("{}%", (r * 100.0).round() as u32))
        .unwrap_or_else(|| "—".into());
    println!(
        "hoje: {} revisões · streak: {} {} · retenção 30d: {ret}",
        today_count,
        streak_days,
        if streak_days == 1 { "dia" } else { "dias" }
    );
    Ok(())
}

fn run_list(root: &Path, deck: Option<&str>) -> Result<(), String> {
    let names: Vec<String> = match deck {
        Some(d) => vec![card::sanitize_deck_name(d)],
        None => card::deck_names(root),
    };
    for name in names {
        let cards = card::load_deck(root, &name)?;
        println!(
            "{} {}",
            ui::cyan(&name),
            ui::dim(&format!("({} cards)", cards.len()))
        );
        for c in &cards {
            let rev = if c.reverse { ui::dim(" ⇄") } else { String::new() };
            let tags = if c.tags.is_empty() {
                String::new()
            } else {
                ui::dim(&format!("  #{}", c.tags.join(" #")))
            };
            let front: String = c.front.chars().take(64).collect();
            println!("  {} {}{}{}", ui::dim(&c.id), front, rev, tags);
        }
    }
    Ok(())
}

/// Ranking de erros do histórico: quais cards merecem ser quebrados em menores.
/// Lê `state/` sem alterar nada; ids de cards removidos aparecem como "— removido".
fn run_errors(root: &Path) -> Result<(), String> {
    let store = Store::load(root);
    // front de todos os cards ativos, indexado pelo id sem o sufixo de direção (#A/#B)
    let mut fronts: HashMap<String, (String, String)> = HashMap::new();
    for (deck, cards) in card::load_decks(root, &[])? {
        for c in cards {
            let front = c.front.lines().next().unwrap_or("").to_string();
            fronts.insert(c.id, (deck.clone(), front));
        }
    }
    // grade 1 = Errei (grades antigas 3/4 também contam como acerto)
    let mut wrong: HashMap<String, u32> = HashMap::new();
    for h in store.history() {
        if h.grade == 1 {
            *wrong.entry(h.id).or_insert(0) += 1;
        }
    }
    if wrong.is_empty() {
        println!("nenhum erro registrado no histórico — follow up com anki stats");
        return Ok(());
    }
    let mut rows: Vec<(String, u32)> = wrong.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    println!("{}  {}", ui::bold("cards mais errados"), ui::dim("(grade Errei no histórico)"));
    let mut total = 0u32;
    for (key, n) in &rows {
        total += n;
        let base = key.split('#').next().unwrap_or(&key);
        match fronts.get(base) {
            Some((deck, front)) => {
                let front: String = front.chars().take(48).collect();
                println!("{:>3}×  {:<24} {:<10} {}", n, key, ui::dim(deck), front);
            }
            None => println!("{:>3}×  {:<24} {}", n, key, ui::dim("— removido")),
        }
    }
    println!(
        "\n{} {} erros em {} itens · reforço livre: {}",
        ui::dim("total:"),
        total,
        rows.len(),
        ui::cyan("anki study --errei")
    );
    Ok(())
}

/// Ação executável pelo painel do `anki` nu.
enum Act {
    ReviewAll,
    ReviewDeck(String),
    Study,
    Erros,
    Due,
    Stats,
    List,
    Add,
}

/// Painel interativo do `anki` sem argumentos: mostra o status de cada deck
/// (vencidos · novos) e roda a ação escolhida, voltando ao menu. Decks
/// esquecidos aparecem aqui todo dia com a fila esperando.
fn run_panel(root: &Path) -> Result<(), String> {
    loop {
        let today = Local::now().date_naive();
        let store = Store::load(root);
        let names = card::deck_names(root);

        // mesmo cálculo do `due`: vencidos (due ≤ hoje) e sem-estado (novos)
        let mut status: Vec<(String, usize, usize)> = Vec::new();
        for name in &names {
            let cards = card::load_deck(root, name)?;
            let keys: Vec<String> = cards.iter().flat_map(card::keys_for).collect();
            let due = keys
                .iter()
                .filter(|k| {
                    store.schedule.get(k.as_str()).is_some_and(|s| s.due <= today)
                })
                .count();
            let new = keys
                .iter()
                .filter(|k| !store.schedule.contains_key(k.as_str()))
                .count();
            status.push((name.clone(), due, new));
        }

        let mut opts: Vec<(Act, String, String)> = Vec::new();
        if !status.is_empty() {
            let (td, tn) = status
                .iter()
                .fold((0usize, 0usize), |(d, n), (_, d2, n2)| (d + d2, n + n2));
            opts.push((
                Act::ReviewAll,
                "revisar tudo".into(),
                format!("{td} vencidos · {tn} novos"),
            ));
            for (name, d, n) in &status {
                opts.push((
                    Act::ReviewDeck(name.clone()),
                    format!("revisar {name}"),
                    format!("{d} vencidos · {n} novos"),
                ));
            }
        }
        opts.push((Act::Study, "study".into(), "reforço livre, sem agendamento".into()));
        opts.push((Act::Erros, "erros".into(), "ranking de erros do histórico".into()));
        opts.push((Act::Due, "due".into(), "tabela de vencidos/novos".into()));
        opts.push((Act::Stats, "stats".into(), "streak e retenção".into()));
        opts.push((Act::List, "list".into(), "listar cards".into()));
        opts.push((Act::Add, "add".into(), "adicionar um card".into()));

        println!("{}", ui::bold("anki — o que fazer?"));
        for (i, (_, label, detail)) in opts.iter().enumerate() {
            println!("{:>3}) {:<18} {}", i + 1, label, ui::dim(detail));
        }
        println!("{:>3}) {}", "q", "sair");

        ui::out(&format!("\n{} ", ui::cyan("escolha:")));
        let choice = match ui::read_line() {
            Ok(Some(c)) => c.trim().to_lowercase(),
            _ => return Ok(()), // EOF (Ctrl-D) encerra
        };
        if choice == "q" {
            return Ok(());
        }
        let idx: usize = match choice.parse() {
            Ok(v) => v,
            Err(_) => {
                println!("{}", ui::yellow("não entendi — digite o número da opção (ou q)"));
                continue;
            }
        };
        let act = match opts.get(idx.wrapping_sub(1)) {
            Some((act, _, _)) => act,
            None => {
                println!("{}", ui::yellow(&format!("opção {idx} fora da lista — 1 a {}", opts.len())));
                continue;
            }
        };
        match act {
            Act::ReviewAll => run_review(root, &[], 10)?,
            Act::ReviewDeck(deck) => run_review(root, &[deck.clone()], 10)?,
            Act::Study => run_study(root, &[], false)?,
            Act::Erros => run_errors(root)?,
            Act::Due => run_overview(root, false)?,
            Act::Stats => run_stats(root)?,
            Act::List => run_list(root, None)?,
            Act::Add => {
                ui::out(&format!("{} ", ui::cyan("deck:")));
                if let Ok(Some(d)) = ui::read_line() {
                    run_add(root, d.trim())?;
                }
            }
        }
        println!();
    }
}
