#!/bin/bash
# Сборка Rectangle 2 (Rust) в .app-бандл.
#
#   ./build.sh             dist/Rectangle2Rust.app; в /Applications не ставит
#   ./build.sh --install   то же и установка в «/Applications/Rectangle 2 (Rust).app»
#   ./build.sh --dev       dist/Rectangle2Rust-dev.app для проверок: bundle id
#                          local.rectangle2rust.dev, имя «Rectangle 2 (Rust, тест)»,
#                          URL-схема rectangle2rust-dev — свои настройки и своё
#                          разрешение доступа, установленную копию не трогает;
#                          dev-вариант не устанавливается никогда
#   ./build.sh --debug     отладочная сборка cargo (можно вместе с --dev)
#   ./build.sh --universal один бинарь для Apple Silicon и Intel (нужен
#                          `rustup target add aarch64-apple-darwin x86_64-apple-darwin`)
#
# Версия: CFBundleShortVersionString — из Cargo.toml, CFBundleVersion — число
# коммитов (git rev-list --count HEAD), без git — 1.
set -euo pipefail
cd "$(dirname "$0")"

INSTALL=0
DEV=0
UNIVERSAL=0
PROFILE=release
for arg in "$@"; do
    case "$arg" in
        --install) INSTALL=1 ;;
        --dev) DEV=1 ;;
        --debug) PROFILE=debug ;;
        --universal) UNIVERSAL=1 ;;
        -h | --help)
            sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "неизвестный параметр: $arg (есть --install, --dev, --debug, --universal)" >&2
            exit 2
            ;;
    esac
done
if [ "$DEV" = 1 ] && [ "$INSTALL" = 1 ]; then
    echo "dev-вариант не устанавливается: --dev и --install вместе нельзя" >&2
    exit 2
fi

if [ "$DEV" = 1 ]; then
    APP="dist/Rectangle2Rust-dev.app"
    BUNDLE_ID="local.rectangle2rust.dev"
    APP_NAME="Rectangle 2 (Rust, тест)"
    URL_SCHEME="rectangle2rust-dev"
else
    APP="dist/Rectangle2Rust.app"
    BUNDLE_ID="local.rectangle2rust"
    APP_NAME="Rectangle 2 (Rust)"
    URL_SCHEME="rectangle2rust"
fi

# Версия пакета — строка version в секции [package].
VERSION="$(awk '
    /^\[package\]/ { in_package = 1; next }
    /^\[/ { in_package = 0 }
    in_package && /^version[[:space:]]*=/ { gsub(/^[^"]*"|".*$/, ""); print; exit }
' Cargo.toml)"
if [ -z "$VERSION" ]; then
    echo "в Cargo.toml нет version в секции [package]" >&2
    exit 1
fi
BUILD_NUMBER="$(git rev-list --count HEAD 2>/dev/null || echo 1)"

CARGO_FLAGS=()
if [ "$PROFILE" = release ]; then
    CARGO_FLAGS+=(--release)
fi

# Бинарь — из каталога, куда cargo собирает на самом деле: CARGO_TARGET_DIR или
# build.target-dir из настроек cargo, иначе ./target. С жёстким target/$PROFILE при
# другом каталоге сборки в бандл молча попал бы старый бинарь.
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | plutil -extract target_directory raw -o - -)"
if [ "$UNIVERSAL" = 1 ]; then
    PARTS=()
    for TRIPLE in aarch64-apple-darwin x86_64-apple-darwin; do
        cargo build ${CARGO_FLAGS[@]+"${CARGO_FLAGS[@]}"} --target "$TRIPLE"
        PARTS+=("$TARGET_DIR/$TRIPLE/$PROFILE/rectangle2rust")
    done
    BINARY="$TARGET_DIR/universal-$PROFILE/rectangle2rust"
    mkdir -p "$(dirname "$BINARY")"
    lipo -create -output "$BINARY" "${PARTS[@]}"
else
    cargo build ${CARGO_FLAGS[@]+"${CARGO_FLAGS[@]}"}
    BINARY="$TARGET_DIR/$PROFILE/rectangle2rust"
fi
if [ ! -x "$BINARY" ]; then
    echo "нет собранного бинаря: $BINARY" >&2
    exit 1
fi
echo "бинарь: $BINARY"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/rectangle2rust"
cp packaging/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
# Иконки пунктов меню и статус-бара — те же картинки, что в Assets оригинала.
cp -R packaging/icons "$APP/Contents/Resources/icons"

PLIST="$APP/Contents/Info.plist"
cp packaging/Info.plist "$PLIST"
plutil -replace CFBundleShortVersionString -string "$VERSION" "$PLIST"
plutil -replace CFBundleVersion -string "$BUILD_NUMBER" "$PLIST"
plutil -replace CFBundleIdentifier -string "$BUNDLE_ID" "$PLIST"
plutil -replace CFBundleName -string "$APP_NAME" "$PLIST"
plutil -replace CFBundleDisplayName -string "$APP_NAME" "$PLIST"
plutil -replace CFBundleURLTypes.0.CFBundleURLName -string "$BUNDLE_ID" "$PLIST"
# Массив схем заменяем целиком: `-replace …Schemes.0` вставляет элемент, а не
# заменяет, и dev-вариант перехватывал бы ещё и ссылки настоящего приложения.
plutil -replace CFBundleURLTypes.0.CFBundleURLSchemes -xml \
    "<array><string>$URL_SCHEME</string></array>" "$PLIST"
plutil -lint "$PLIST" >/dev/null
SCHEMES="$(plutil -extract CFBundleURLTypes json -o - "$PLIST")"
EXPECTED_SCHEMES="[{\"CFBundleURLName\":\"$BUNDLE_ID\",\"CFBundleURLSchemes\":[\"$URL_SCHEME\"]}]"
if [ "$SCHEMES" != "$EXPECTED_SCHEMES" ]; then
    echo "в Info.plist не те URL-схемы: $SCHEMES (ждали $EXPECTED_SCHEMES)" >&2
    exit 1
fi
echo "версия $VERSION ($BUILD_NUMBER), $BUNDLE_ID, схема $URL_SCHEME://"

# Подпись. Ad-hoc годится только для проверки: TCC привязывает разрешение
# к хешу подписи, поэтому после каждой пересборки система спрашивает доступ заново.
# Сертификат (Apple Development / Developer ID) даёт стабильную личность — разрешение
# переживает пересборки. Так же подписана установленная Rectangle 2.
# Сертификат задаётся SHA-1 отпечатком, а не именем: после продления в связке бывают
# два действующих сертификата с одним именем, и `codesign --sign <имя>` падает
# с «ambiguous». SIGN_ID из окружения передаётся как есть (имя или отпечаток).
SIGN_ID="${SIGN_ID:-}"
SIGN_NAME=""
if [ -z "$SIGN_ID" ]; then
    # Строка вида `  1) <SHA-1> "Apple Development: … (TEAM)"`.
    IDENTITY="$(security find-identity -v -p codesigning 2>/dev/null \
        | grep -E '"(Apple Development|Developer ID Application|Mac Developer)' \
        | head -1 || true)"
    SIGN_ID="$(printf '%s\n' "$IDENTITY" | awk '{ print $2 }')"
    SIGN_NAME="$(printf '%s\n' "$IDENTITY" | sed -n 's/.*"\(.*\)".*/\1/p')"
    if ! [[ "$SIGN_ID" =~ ^[0-9A-F]{40}$ ]]; then
        SIGN_ID=""
    fi
fi

if [ -n "$SIGN_ID" ]; then
    if [ -n "$SIGN_NAME" ]; then
        echo "подпись: $SIGN_NAME ($SIGN_ID)"
    else
        echo "подпись: $SIGN_ID"
    fi
    codesign --force --sign "$SIGN_ID" --timestamp=none "$APP" 2>&1 | sed 's/^/    /'
else
    echo "подпись: ad-hoc (сертификат не найден — разрешение придётся выдавать после каждой сборки)"
    codesign --force --sign - --timestamp=none "$APP" 2>&1 | sed 's/^/    /'
fi
codesign --verify --verbose=1 "$APP" 2>&1 | sed 's/^/    /' || true
# Сбрасываем кэш иконок, иначе Finder может показать старую болванку.
touch "$APP"

if [ "$INSTALL" = 1 ]; then
    INSTALLED="/Applications/Rectangle 2 (Rust).app"
    # Через временную копию: если копирование упадёт, прежняя установка цела.
    rm -rf "$INSTALLED.new"
    cp -R "$APP" "$INSTALLED.new"
    rm -rf "$INSTALLED"
    mv "$INSTALLED.new" "$INSTALLED"
    touch "$INSTALLED"
    # Регистрация в LaunchServices — чтобы сразу заработала URL-схема.
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
        -f "$INSTALLED" 2>/dev/null || true
    echo "установлено: $INSTALLED (запущенную копию перезапустите)"
fi

echo "готово: $APP"
