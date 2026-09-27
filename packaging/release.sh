#!/bin/bash
# Файлы релиза для GitHub: универсальное приложение (Apple Silicon + Intel) в zip и dmg.
#
#   packaging/release.sh   → dist/Rectangle2Rust.zip, dist/Rectangle2Rust.dmg и их SHA-256
#
# Имена файлов постоянные: на них ведут ссылки releases/latest/download/… в README
# и install.sh. Подпись ad-hoc — личный сертификат в публичную сборку не попадает.
# Для dmg нужен dmgbuild (pip install dmgbuild), без него собирается только zip.
set -euo pipefail
cd "$(dirname "$0")/.."

SIGN_ID="${SIGN_ID:--}" ./build.sh --universal

NAME="Rectangle 2 (Rust).app"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
ditto dist/Rectangle2Rust.app "$STAGE/$NAME"

rm -f dist/Rectangle2Rust.zip dist/Rectangle2Rust.dmg
ditto -c -k --keepParent "$STAGE/$NAME" dist/Rectangle2Rust.zip

DMGBUILD="${DMGBUILD:-$(command -v dmgbuild || true)}"
if [ -n "$DMGBUILD" ]; then
    "$DMGBUILD" -s packaging/dmg/settings.py -D app="$STAGE/$NAME" "Rectangle 2 (Rust)" dist/Rectangle2Rust.dmg
else
    echo "dmgbuild не найден (pip install dmgbuild) — dmg пропущен" >&2
fi

echo
shasum -a 256 dist/Rectangle2Rust.zip dist/Rectangle2Rust.dmg 2>/dev/null || true
