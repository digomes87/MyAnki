# anki

Repetição espaçada no terminal — decks curados em JSONL, revisão por sua conta.

## Uso

```sh
./anki.sh <comando>          # compila se necessário e roda
anki <comando>               # via symlink ~/.local/bin/anki
```

| Comando | O que faz |
|---|---|
| `review [decks...]` | Sessão de revisão (padrão; `--new N` ajusta o teto de estreias/dia) |
| `study [decks...]` | Estudo livre: embaralhado, loop infinito, sem agendamento |
| `add <deck>` | Adiciona um card interativamente |
| `due` | Vencidos e novos por deck |
| `stats` | Streak, retenção, revisões de hoje |
| `list [deck]` | Lista os cards |

Avaliação binária em cada card: `1` ✗ Errei · `2` ✓ Acertei (Enter = Acertei).
Errei volta ao fim da sessão e reagenda para o dia seguinte; Acertei progride o intervalo.

## Formato dos decks

Um JSONL por idioma/tema em `decks/`:

```json
{"id":"italiano-0001","front":"ciao","back":"oi / tchau (informal)","tags":["saudacoes"],"reverse":true,"typed":true,"note":"seed inicial"}
```

| Campo | |
|---|---|
| `id` | `<deck>-<NNNN>`, sequencial e único |
| `front` / `back` | IT→PT nos decks de idioma; `typed` usa a 1ª linha do lado visível como prompt |
| `reverse` | Gera também a direção inversa (`#A` / `#B`) |
| `typed` | Resposta digitada — tolera typo/acento (Levenshtein) |
| `note` | Contexto/origem do card |

## Agendamento

SM-2 simplificado, binário: acerto mantém o ease e multiplica o intervalo; erro desce
o ease (−0.20, piso 1.3), reseta o intervalo para 1d e conta lapse. O progresso vive
em `state/` (`schedule.json` + `history.jsonl`), versionado junto — conteúdo e
agendamento são separados: `decks/` é curadoria, `state/` é histórico.

## Estrutura

```
anki.sh        wrapper: compila se preciso e roda (à prova de symlink)
decks/*.jsonl  conteúdo — um arquivo por deck
src/           CLI em Rust (clap): card, srs, state, answer, ui
state/         agendamento e histórico de revisões (versionado)
```
