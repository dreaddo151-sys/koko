#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/integrations/pstack/upstream"
REPO="https://github.com/cursor/plugins.git"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/dread-pstack.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
if [[ -e "$DEST" ]]; then
  echo "Refusing to overwrite existing pstack import: $DEST" >&2
  exit 2
fi
git clone --depth 1 --filter=blob:none --sparse "$REPO" "$TMP/plugins"
git -C "$TMP/plugins" sparse-checkout set pstack
test -f "$TMP/plugins/pstack/plugin.json"
test -f "$TMP/plugins/pstack/LICENSE"
python3 - "$TMP/plugins/pstack/plugin.json" <<'PY'
import json,sys
m=json.load(open(sys.argv[1]))
assert m.get("name")=="pstack"
assert m.get("license")=="MIT"
print("Validated pstack",m.get("version"),"license",m.get("license"))
PY
mkdir -p "$(dirname "$DEST")"
cp -a "$TMP/plugins/pstack" "$DEST"
echo "Imported to $DEST; no imported code was executed."
