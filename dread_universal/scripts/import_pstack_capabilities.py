#!/usr/bin/env python3
"""Import pinned Pstack metadata into DREAD's disabled-by-default capability registry.

This tool never executes imported skills, agents, hooks, scripts, or automations.
It defaults to a local validation/dry-run; POST requests require --apply.
"""
from __future__ import annotations

import argparse
import hashlib
import ipaddress
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any


class ImportErrorSafe(RuntimeError):
    pass


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def _is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def validate_loopback_api_url(raw: str) -> str:
    parsed = urllib.parse.urlsplit(raw)
    try:
        port = parsed.port
    except ValueError as exc:
        raise ImportErrorSafe("API URL contains an invalid port") from exc
    if parsed.scheme != "http" or not parsed.hostname or port is None:
        raise ImportErrorSafe("API URL must be explicit-port loopback HTTP")
    if parsed.username is not None or parsed.password is not None or parsed.query or parsed.fragment:
        raise ImportErrorSafe("API URL cannot contain credentials, query strings or fragments")
    host = parsed.hostname.lower()
    if host != "localhost":
        try:
            if not ipaddress.ip_address(host).is_loopback:
                raise ImportErrorSafe("API URL host must be loopback")
        except ValueError as exc:
            raise ImportErrorSafe("API URL host must be localhost or a loopback IP") from exc
    if parsed.path not in ("", "/"):
        raise ImportErrorSafe("API base URL must not include a path")
    return raw.rstrip("/")


def _safe_source_file(source_root: Path, relative: str) -> Path:
    rel = Path(relative)
    if rel.is_absolute() or ".." in rel.parts:
        raise ImportErrorSafe(f"Unsafe catalog path: {relative}")
    candidate = (source_root / rel).resolve()
    if not _is_within(candidate, source_root.resolve()) or not candidate.is_file():
        raise ImportErrorSafe(f"Catalog source file is missing or escapes the pinned tree: {relative}")
    return candidate


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_import_records(project_root: Path) -> list[dict[str, Any]]:
    integration_root = project_root / "integrations" / "pstack"
    catalog_path = integration_root / "catalog.json"
    source_root = integration_root / "upstream"
    marker_path = source_root / ".dread-import.json"
    manifest_path = source_root / "plugin.json"
    license_path = source_root / "LICENSE"

    for path in (catalog_path, marker_path, manifest_path, license_path):
        if not path.is_file():
            raise ImportErrorSafe(f"Required pinned Pstack file is missing: {path}")

    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    marker = json.loads(marker_path.read_text(encoding="utf-8"))
    upstream = json.loads(manifest_path.read_text(encoding="utf-8"))
    if catalog.get("schema_version") != 1 or catalog.get("name") != "pstack":
        raise ImportErrorSafe("Unsupported Pstack catalog schema or name")

    if marker.get("schema_version") != 1 or marker.get("verified_by") != "scripts/import-pstack.sh":
        raise ImportErrorSafe("Pstack source is missing the expected pinned-import verification marker")

    expected = {
        "repository": catalog.get("repository"),
        "commit_sha": catalog.get("upstream_commit_sha"),
        "tree_sha": catalog.get("upstream_tree_sha"),
        "manifest_version": catalog.get("upstream_manifest_version"),
        "license": catalog.get("license"),
    }
    for key, value in expected.items():
        if not value or marker.get(key) != value:
            raise ImportErrorSafe(f"Pinned Pstack import metadata mismatch for {key}")

    if upstream.get("name") != "pstack":
        raise ImportErrorSafe("Imported upstream manifest is not pstack")
    if upstream.get("version") != expected["manifest_version"]:
        raise ImportErrorSafe("Imported Pstack version does not match pinned catalog")
    if upstream.get("license") != expected["license"]:
        raise ImportErrorSafe("Imported Pstack license does not match pinned catalog")

    skills = catalog.get("skills")
    agents = catalog.get("agents", [])
    if not isinstance(skills, list) or len(skills) != catalog.get("inventory_count"):
        raise ImportErrorSafe("Pstack skill inventory count does not match catalog")
    if not isinstance(agents, list):
        raise ImportErrorSafe("Pstack agents inventory must be an array")

    records: list[dict[str, Any]] = []
    seen: set[str] = set()
    imported_at = int(time.time())
    for kind, entries, prefix in (("skill", skills, "pstack.skill"), ("agent", agents, "pstack.agent")):
        for entry in entries:
            item_id = entry.get("id")
            relative = entry.get("path")
            compatibility = entry.get("initial_compatibility", "requires_review")
            if not isinstance(item_id, str) or not item_id or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789-_" for c in item_id):
                raise ImportErrorSafe(f"Invalid Pstack {kind} identifier: {item_id!r}")
            capability_id = f"{prefix}.{item_id}"
            if capability_id in seen:
                raise ImportErrorSafe(f"Duplicate Pstack capability ID: {capability_id}")
            seen.add(capability_id)
            if not isinstance(relative, str) or not relative.startswith(("skills/" if kind == "skill" else "agents/")):
                raise ImportErrorSafe(f"Invalid Pstack {kind} source path: {relative!r}")
            source_file = _safe_source_file(source_root, relative)
            content_hash = _sha256(source_file)
            pinned_hash = marker.get("files_sha256", {}).get(relative)
            if not pinned_hash or pinned_hash != content_hash:
                raise ImportErrorSafe(f"Pinned Pstack file hash mismatch: {relative}")
            content = source_file.read_text(encoding="utf-8", errors="replace")
            heading = next((line.lstrip("# ").strip() for line in content.splitlines() if line.startswith("# ")), item_id)
            source_uri = f"{expected['repository'].rstrip('/')}/tree/{expected['commit_sha']}/pstack/{relative}"
            records.append({
                "id": capability_id,
                "name": heading[:200] or item_id,
                "kind": kind,
                "description": f"Imported Pstack {kind} metadata for review. Source content is not executed.",
                "version": expected["manifest_version"],
                "license": expected["license"],
                "provenance": {
                    "source_name": "Pstack",
                    "source_uri": source_uri,
                    "source_version": expected["manifest_version"],
                    "source_revision": expected["commit_sha"],
                    "imported_at": imported_at,
                },
                "compatibility": compatibility,
                "dependencies": [],
                "adapter": None,
                "manifest": {
                    "source_path": relative,
                    "source_sha256": content_hash,
                    "source_tree_sha": expected["tree_sha"],
                    "initial_compatibility": compatibility,
                    "review_state": "disabled_until_reviewed",
                    "execution_performed": False,
                },
            })
    return records


def _request(opener: urllib.request.OpenerDirector, url: str, method: str, payload: dict | None = None) -> tuple[int, dict | None]:
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(url, data=data, method=method, headers={"Accept": "application/json", **({"Content-Type": "application/json"} if data is not None else {})})
    try:
        with opener.open(request, timeout=3) as response:
            body = response.read(1024 * 1024)
            return response.status, json.loads(body) if body else None
    except urllib.error.HTTPError as exc:
        body = exc.read(1024 * 1024)
        parsed = None
        if body:
            try:
                parsed = json.loads(body)
            except json.JSONDecodeError:
                parsed = None
        return exc.code, parsed


def _require_disabled_existing(body: dict | None, record: dict[str, Any]) -> None:
    capability_id = record["id"]
    item = body.get("item") if isinstance(body, dict) else None
    if not isinstance(item, dict) or item.get("status") != "disabled":
        raise ImportErrorSafe(f"Existing capability {capability_id} is not confirmed disabled; refusing to overwrite or skip it")
    existing_provenance = item.get("provenance")
    expected_provenance = record["provenance"]
    if not isinstance(existing_provenance, dict) or any(
        existing_provenance.get(key) != expected_provenance.get(key)
        for key in ("source_name", "source_revision")
    ):
        raise ImportErrorSafe(f"Existing capability {capability_id} has different provenance; refusing to overwrite or skip it")


def apply_records(records: list[dict[str, Any]], api_base: str) -> tuple[int, int]:
    base = validate_loopback_api_url(api_base)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    created = skipped = 0
    for record in records:
        item_url = f"{base}/api/capabilities/{urllib.parse.quote(record['id'], safe='.-_')}"
        status, body = _request(opener, item_url, "GET")
        if status == 200:
            _require_disabled_existing(body, record)
            skipped += 1
            continue
        if status != 404:
            raise ImportErrorSafe(f"Capability lookup failed for {record['id']}: HTTP {status}")
        status, body = _request(opener, f"{base}/api/capabilities", "POST", record)
        if status == 201:
            created += 1
        elif status == 409:
            existing_status, existing_body = _request(opener, item_url, "GET")
            if existing_status != 200:
                raise ImportErrorSafe(f"Capability conflict could not be safely verified for {record['id']}")
            _require_disabled_existing(existing_body, record)
            skipped += 1
        else:
            raise ImportErrorSafe(f"Capability import failed for {record['id']}: HTTP {status} {body}")
    return created, skipped


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--api-base", default="http://127.0.0.1:8765")
    parser.add_argument("--apply", action="store_true", help="POST disabled capability metadata to the loopback API")
    args = parser.parse_args(argv)
    try:
        records = load_import_records(args.root.resolve())
        skills = sum(item["kind"] == "skill" for item in records)
        agents = len(records) - skills
        print(f"Validated pinned Pstack inventory: {skills} skills, {agents} agents; all start disabled.")
        if not args.apply:
            print("Dry run only. Re-run with --apply to register metadata; no imported content will execute.")
            return 0
        created, skipped = apply_records(records, args.api_base)
        print(f"Capability metadata import complete: created={created}, already_present={skipped}; execution=none.")
        return 0
    except (ImportErrorSafe, OSError, json.JSONDecodeError, urllib.error.URLError) as exc:
        print(f"Pstack capability import refused: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
