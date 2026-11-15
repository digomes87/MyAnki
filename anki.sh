#!/usr/bin/env bash
# anki.sh — compila se necessário e roda a sessão de revisão
# uso: ./anki.sh [review decks...] [--new N] | add <deck> | due | stats | list
set -euo pipefail
cd "$(dirname "$0")"

BIN="target/release/anki"

# rebuilda quando fontes forem mais novas que o binário (ou ele não existir)
if [ ! -x "$BIN" ] || [ -n "$(find src Cargo.toml -newer "$BIN" -print -quit 2>/dev/null)" ]; then
  cargo build --release
fi

exec "$BIN" "$@"
