#!/bin/bash
# Установка Rectangle 2 (Rust) одной командой:
#   curl -fsSL https://raw.githubusercontent.com/Lutamona/Rectangle2Rust/main/install.sh | bash
set -euo pipefail

URL="https://github.com/Lutamona/Rectangle2Rust/releases/latest/download/Rectangle2Rust.zip"
APP="/Applications/Rectangle 2 (Rust).app"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Скачиваю последнюю версию…"
curl -fsSL "$URL" -o "$TMP/app.zip"
ditto -x -k "$TMP/app.zip" "$TMP"

# Если приложение уже запущено — закрываем, чтобы заменить.
pkill -x rectangle2rust 2>/dev/null || true

echo "Ставлю в /Applications…"
rm -rf "$APP"
ditto "$TMP/Rectangle 2 (Rust).app" "$APP"
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

open "$APP"
echo "Готово! Иконка появилась в строке меню справа вверху."
echo "Дай доступ: Системные настройки → Конфиденциальность и безопасность → Универсальный доступ."
