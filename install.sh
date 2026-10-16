#!/bin/sh
# Installs umlaut as a local Typst package, so it can be imported as
# `@local/umlaut:<version>` without going through Typst Universe.
#
#   curl -fsSL https://raw.githubusercontent.com/robertjndw/umlaut/main/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --ref v0.1.0
#   ./install.sh                  # from a clone: installs your working tree
#
# Options:
#   --ref <branch|tag>    what to download from GitHub (default: main)
#   --namespace <name>    package namespace (default: local)
#   --source <dir>        install from this directory instead of downloading
#
# See https://github.com/typst/packages#local-packages for where Typst looks.

set -eu

REPO="robertjndw/umlaut"
REF="${UMLAUT_REF:-main}"
NAMESPACE="${UMLAUT_NAMESPACE:-local}"
SOURCE=""

# Only what the published package contains. Keep in sync with `exclude` in typst.toml.
FILES="typst.toml umlaut.wasm src README.md LICENSE THIRD-PARTY-NOTICES.md CHANGELOG.md"

die() {
    echo "error: $*" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --ref) [ $# -ge 2 ] || die "--ref needs a value"; REF="$2"; shift 2 ;;
        --namespace) [ $# -ge 2 ] || die "--namespace needs a value"; NAMESPACE="$2"; shift 2 ;;
        --source) [ $# -ge 2 ] || die "--source needs a value"; SOURCE="$2"; shift 2 ;;
        -h | --help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
done

# When run as a file from a checkout, install that checkout. Piped into sh, $0 is the
# shell itself and there is nothing next to it, so we download instead.
if [ -z "$SOURCE" ] && [ -f "$0" ] && [ -f "$(dirname "$0")/typst.toml" ]; then
    SOURCE="$(dirname "$0")"
fi

package_dir() {
    if [ -n "${TYPST_PACKAGE_PATH:-}" ]; then
        echo "$TYPST_PACKAGE_PATH"
        return
    fi
    # Ask typst first, it knows about Snap and other non-default setups. Older
    # versions don't have `info --format json`, in which case we fall through.
    if command -v typst >/dev/null 2>&1; then
        path="$(typst info --format json 2>/dev/null \
            | sed -n 's/.*"package-path":"\([^"]*\)".*/\1/p' | sed 's/\\\\/\\/g')"
        if [ -n "$path" ]; then
            echo "$path"
            return
        fi
    fi
    case "$(uname -s)" in
        Darwin) echo "$HOME/Library/Application Support/typst/packages" ;;
        MINGW* | MSYS* | CYGWIN*) echo "${APPDATA:?APPDATA is not set}/typst/packages" ;;
        *) echo "${XDG_DATA_HOME:-$HOME/.local/share}/typst/packages" ;;
    esac
}

TMP=""
cleanup() { if [ -n "$TMP" ]; then rm -rf "$TMP"; fi; }
trap cleanup EXIT INT TERM

if [ -z "$SOURCE" ]; then
    command -v curl >/dev/null 2>&1 || die "curl is required"
    command -v tar >/dev/null 2>&1 || die "tar is required"
    TMP="$(mktemp -d)"
    # GitHub resolves branches and tags under the same archive URL.
    url="https://codeload.github.com/$REPO/tar.gz/$REF"
    echo "Downloading $REPO@$REF"
    curl -fsSL "$url" -o "$TMP/src.tar.gz" || die "could not download $url (does the branch or tag exist?)"
    tar -xzf "$TMP/src.tar.gz" -C "$TMP"
    SOURCE="$(find "$TMP" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
fi

[ -f "$SOURCE/typst.toml" ] || die "no typst.toml in $SOURCE"
NAME="$(sed -n 's/^name *= *"\(.*\)"/\1/p' "$SOURCE/typst.toml" | head -n 1)"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$SOURCE/typst.toml" | head -n 1)"
[ -n "$NAME" ] && [ -n "$VERSION" ] || die "could not read name and version from typst.toml"

TARGET="$(package_dir)/$NAMESPACE/$NAME/$VERSION"
# Replace rather than merge, so files removed upstream don't linger.
rm -rf "$TARGET"
mkdir -p "$TARGET"
for f in $FILES; do
    [ -e "$SOURCE/$f" ] || die "$f is missing from $SOURCE"
    cp -R "$SOURCE/$f" "$TARGET/"
done

echo "Installed $NAME $VERSION to $TARGET"
echo "Use it with: #import \"@$NAMESPACE/$NAME:$VERSION\": drawio, plantuml"
