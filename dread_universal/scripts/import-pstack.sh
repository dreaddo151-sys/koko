#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/integrations/pstack/upstream"
REPO="https://github.com/cursor/plugins.git"
EXPECTED_COMMIT="2f87c854fe3e2fcda44ca86b46ede35aad3c46b7"
EXPECTED_TREE="71d8e1d1d718e2149dc43b2535618064f29cba6b"
EXPECTED_VERSION="0.15.43"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/dread-pstack.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
if [[ -e "$DEST" ]]; then
  echo "Refusing to overwrite existing pstack import: $DEST" >&2
  exit 2
fi
git init "$TMP/plugins"
git -C "$TMP/plugins" remote add origin "$REPO"
git -C "$TMP/plugins" fetch --depth 1 --filter=blob:none origin "$EXPECTED_COMMIT"
git -C "$TMP/plugins" sparse-checkout init --cone
git -C "$TMP/plugins" sparse-checkout set pstack
git -C "$TMP/plugins" checkout --detach FETCH_HEAD
ACTUAL_COMMIT="$(git -C "$TMP/plugins" rev-parse HEAD)"
ACTUAL_TREE="$(git -C "$TMP/plugins" rev-parse 'HEAD:pstack')"
if [[ "$ACTUAL_COMMIT" != "$EXPECTED_COMMIT" || "$ACTUAL_TREE" != "$EXPECTED_TREE" ]]; then
  echo "Upstream pstack revision mismatch: commit=$ACTUAL_COMMIT tree=$ACTUAL_TREE" >&2
  exit 3
fi
test -f "$TMP/plugins/pstack/plugin.json"
test -f "$TMP/plugins/pstack/LICENSE"
EXPECTED_VERSION="$EXPECTED_VERSION" python3 - "$TMP/plugins/pstack/plugin.json" <<'PY'
import json,os,sys
m=json.load(open(sys.argv[1]))
assert m.get("name")=="pstack"
assert m.get("license")=="MIT"
assert m.get("version")==os.environ["EXPECTED_VERSION"], m.get("version")
print("Validated pstack",m.get("version"),"license",m.get("license"))
PY
mkdir -p "$(dirname "$DEST")"
cp -a "$TMP/plugins/pstack" "$DEST"
python3 - "$DEST" "$ROOT/integrations/pstack/catalog.json" "${REPO%.git}" "$ACTUAL_COMMIT" "$ACTUAL_TREE" "$EXPECTED_VERSION" <<'PY'
import hashlib, json, sys
from pathlib import Path
source_root, catalog_path = Path(sys.argv[1]), Path(sys.argv[2])
catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
entries = catalog.get("skills", []) + catalog.get("agents", [])
file_hashes = {}
for entry in entries:
    relative = Path(entry["path"])
    if relative.is_absolute() or ".." in relative.parts:
        raise SystemExit("Unsafe path in Pstack catalog: " + str(relative))
    source = (source_root / relative).resolve()
    if not source.is_relative_to(source_root.resolve()) or not source.is_file():
        raise SystemExit("Missing or unsafe Pstack source file: " + str(relative))
    file_hashes[relative.as_posix()] = hashlib.sha256(source.read_bytes()).hexdigest()
metadata = {
    "schema_version": 1,
    "repository": sys.argv[3],
    "commit_sha": sys.argv[4],
    "tree_sha": sys.argv[5],
    "manifest_version": sys.argv[6],
    "license": "MIT",
    "verified_by": "scripts/import-pstack.sh",
    "files_sha256": file_hashes,
}
(source_root / ".dread-import.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
PY
echo "Imported to $DEST; pinned commit/tree recorded; no imported code was executed."
