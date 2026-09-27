#!/bin/bash
# Установка и обновление Rectangle 2 (Rust) одной командой:
#
#   curl -fsSL https://raw.githubusercontent.com/Lutamona/Rectangle2Rust/main/install.sh | bash
#
# Скачивает последний релиз с GitHub и кладёт приложение в /Applications (если туда
# нельзя писать — в ~/Applications). Скачанное через curl macOS не помечает как
# «файл из интернета», поэтому предупреждения о непроверенном разработчике не будет.
# Другая папка: APP_DIR=~/Desktop bash install.sh
set -euo pipefail

URL="https://github.com/Lutamona/Rectangle2Rust/releases/latest/download/Rectangle2Rust.zip"
NAME="Rectangle 2 (Rust).app"
BUNDLE_ID="local.rectangle2rust"
PROCESS="rectangle2rust"

if [ -t 1 ]; then
    BOLD=$'\033[1m' GREEN=$'\033[32m' RED=$'\033[31m' RESET=$'\033[0m'
else
    BOLD="" GREEN="" RED="" RESET=""
fi
fail() {
    echo "${RED}Ошибка:${RESET} $*" >&2
    exit 1
}

[ "$(uname -s)" = Darwin ] || fail "Rectangle 2 работает только на macOS."
MACOS="$(sw_vers -productVersion)"
[ "${MACOS%%.*}" -ge 11 ] || fail "нужна macOS 11 Big Sur или новее, а у вас $MACOS."

# Куда ставить: туда, где программа уже стоит, иначе в /Applications, а без прав
# на неё — в ~/Applications.
if [ -n "${APP_DIR:-}" ]; then
    DIR="$APP_DIR"
elif [ -d "/Applications/$NAME" ] || { [ ! -d "$HOME/Applications/$NAME" ] && [ -w /Applications ]; }; then
    DIR="/Applications"
else
    DIR="$HOME/Applications"
fi
{ mkdir -p "$DIR" 2>/dev/null && [ -w "$DIR" ]; } || fail "нет прав на запись в $DIR. Поставьте в свою папку программ:
  curl -fsSL https://raw.githubusercontent.com/Lutamona/Rectangle2Rust/main/install.sh | APP_DIR=~/Applications bash"
APP="$DIR/$NAME"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "${BOLD}Скачиваю Rectangle 2 (Rust)…${RESET}"
curl -fL --progress-bar "$URL" -o "$TMP/app.zip" || fail "не удалось скачать $URL"
ditto -x -k "$TMP/app.zip" "$TMP" || fail "архив повреждён, запустите установку ещё раз."
[ -d "$TMP/$NAME" ] || fail "в архиве нет $NAME."
codesign --verify "$TMP/$NAME" 2>/dev/null || fail "подпись приложения не сходится, архив повреждён."
VERSION="$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$TMP/$NAME/Contents/Info.plist")"

# Подпись без платного сертификата Apple своя у каждой версии, а macOS привязывает
# разрешение «Универсальный доступ» к подписи. После обновления старая галочка
# в настройках горит, но уже не действует — сбрасываем её, и программа спросит
# доступ заново. Та же версия поверх себя разрешение не теряет.
cdhash() { codesign -dv --verbose=4 "$1" 2>&1 | sed -n 's/^CDHash=//p' | head -1; }
FRESH=0
RESET_ACCESS=0
if [ -d "$APP" ]; then
    echo "Обновляю $APP до версии ${VERSION}…"
    [ "$(cdhash "$APP")" = "$(cdhash "$TMP/$NAME")" ] || RESET_ACCESS=1
else
    echo "Ставлю версию $VERSION в ${DIR}…"
    FRESH=1
fi

# Запущенную копию закрываем, иначе файлы не заменить.
if pgrep -x "$PROCESS" >/dev/null; then
    pkill -x "$PROCESS" || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        pgrep -x "$PROCESS" >/dev/null || break
        sleep 0.5
    done
fi

# Через временную копию: если копирование упадёт, прежняя установка цела.
rm -rf "$APP.new"
ditto "$TMP/$NAME" "$APP.new"
rm -rf "$APP"
mv "$APP.new" "$APP"
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true
if [ "$RESET_ACCESS" = 1 ]; then
    tccutil reset Accessibility "$BUNDLE_ID" >/dev/null 2>&1 || true
fi

open "$APP" 2>/dev/null || echo "Запустите «Rectangle 2 (Rust)» из $DIR."
echo
echo "${GREEN}${BOLD}Готово!${RESET} Rectangle 2 (Rust) $VERSION — в $DIR, значок появится в строке меню справа вверху."
if [ "$FRESH" = 1 ] || [ "$RESET_ACCESS" = 1 ]; then
    if [ "$RESET_ACCESS" = 1 ]; then
        echo "После обновления macOS спрашивает разрешение заново — это нормально."
    fi
    echo "Последний шаг — разрешить управление окнами: программа сама покажет окно с кнопкой,"
    echo "или откройте Системные настройки → Конфиденциальность и безопасность → Универсальный доступ."
fi
