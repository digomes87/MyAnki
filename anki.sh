#!/usr/bin/env bash
# anki.sh — compila se necessário e roda a sessão de revisão
# uso: anki (painel interativo) | review [decks...] [--new N] | study [decks...] | add <deck> | due | stats | list | erros
# funciona de qualquer diretório e via symlink (ex.: ~/.local/bin/anki)
set -euo pipefail
SELF="$(readlink -f "$0" 2>/dev/null || echo "$0")"
cd "$(dirname "$SELF")"

BIN="target/release/anki"

# rebuilda quando fontes forem mais novas que o binário (ou ele não existir)
if [ ! -x "$BIN" ] || [ -n "$(find src Cargo.toml -newer "$BIN" -print -quit 2>/dev/null)" ]; then
  cargo build --release
fi

exec "$BIN" "$@"
