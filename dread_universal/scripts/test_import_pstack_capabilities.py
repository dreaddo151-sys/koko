import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import import_pstack_capabilities as importer


class PstackCapabilityImportTests(unittest.TestCase):
    def make_fixture(self, root: Path) -> Path:
        project_root = Path(__file__).resolve().parents[1]
        source = project_root / "integrations" / "pstack"
        destination = root / "integrations" / "pstack"
        shutil.copytree(source, destination)
        return root

    def test_builds_disabled_metadata_records_for_skills_and_agents(self):
        with tempfile.TemporaryDirectory() as tmp:
            records = importer.load_import_records(self.make_fixture(Path(tmp)))
        self.assertEqual(len(records), 60)
        self.assertEqual(sum(item["kind"] == "skill" for item in records), 58)
        self.assertEqual(sum(item["kind"] == "agent" for item in records), 2)
        self.assertEqual({item["kind"] for item in records}, {"skill", "agent"})
        self.assertTrue(all("status" not in item for item in records))
        self.assertTrue(all(item["manifest"]["execution_performed"] is False for item in records))
        self.assertEqual(records[0]["provenance"]["source_revision"], importer.PINNED_COMMIT)

    def test_refuses_unverified_source_snapshot(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.make_fixture(Path(tmp))
            (root / "integrations/pstack/upstream/.dread-import.json").unlink()
            with self.assertRaises(importer.ImportErrorSafe):
                importer.load_import_records(root)

    def test_rejects_pinned_revision_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.make_fixture(Path(tmp))
            marker = root / "integrations/pstack/upstream/.dread-import.json"
            data = json.loads(marker.read_text())
            data["commit_sha"] = "c" * 40
            marker.write_text(json.dumps(data))
            with self.assertRaises(importer.ImportErrorSafe):
                importer.load_import_records(root)

    def test_rejects_tampered_catalog_even_when_revision_fields_are_preserved(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.make_fixture(Path(tmp))
            catalog_path = root / "integrations/pstack/catalog.json"
            data = json.loads(catalog_path.read_text())
            data["skills"][0]["path"] = "skills/../../outside/SKILL.md"
            catalog_path.write_text(json.dumps(data))
            with self.assertRaisesRegex(importer.ImportErrorSafe, "catalog hash"):
                importer.load_import_records(root)

    def test_safe_source_file_rejects_symlink_escape(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_root = root / "upstream"
            source_root.mkdir()
            outside = root / "outside.md"
            outside.write_text("# outside")
            (source_root / "escape.md").symlink_to(outside)
            with self.assertRaises(importer.ImportErrorSafe):
                importer._safe_source_file(source_root, "escape.md")

    def test_rejects_path_traversal(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.make_fixture(Path(tmp))
            catalog_path = root / "integrations/pstack/catalog.json"
            data = json.loads(catalog_path.read_text())
            data["skills"][0]["path"] = "skills/../../outside/SKILL.md"
            catalog_path.write_text(json.dumps(data))
            with self.assertRaises(importer.ImportErrorSafe):
                importer.load_import_records(root)

    def test_rejects_source_file_changed_after_pinned_import(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.make_fixture(Path(tmp))
            source = root / "integrations/pstack/upstream/skills/example-skill/SKILL.md"
            source.write_text("# Tampered after import")
            with self.assertRaises(importer.ImportErrorSafe):
                importer.load_import_records(root)

    def test_rejects_manifest_version_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.make_fixture(Path(tmp))
            manifest = root / "integrations/pstack/upstream/plugin.json"
            manifest.write_text(json.dumps({"name": "pstack", "version": "0.9.0", "license": "MIT"}))
            with self.assertRaises(importer.ImportErrorSafe):
                importer.load_import_records(root)

    def test_api_url_must_be_loopback_and_explicit_port(self):
        self.assertEqual(importer.validate_loopback_api_url("http://127.0.0.1:8765"), "http://127.0.0.1:8765")
        with self.assertRaises(importer.ImportErrorSafe):
            importer.validate_loopback_api_url("http://192.0.2.1:8765")
        with self.assertRaises(importer.ImportErrorSafe):
            importer.validate_loopback_api_url("http://127.0.0.1/api")

    def test_existing_capability_must_be_disabled_and_match_provenance(self):
        record = {"id": "pstack.skill.example", "provenance": {"source_name": "Pstack", "source_revision": "a" * 40}}
        existing = {"item": {"status": "disabled", "provenance": {"source_name": "Pstack", "source_revision": "a" * 40}}}
        importer._require_disabled_existing(existing, record)
        enabled = {"item": {"status": "enabled", "provenance": existing["item"]["provenance"]}}
        with self.assertRaises(importer.ImportErrorSafe):
            importer._require_disabled_existing(enabled, record)
        stale = {"item": {"status": "disabled", "provenance": {"source_name": "Pstack", "source_revision": "b" * 40}}}
        with self.assertRaises(importer.ImportErrorSafe):
            importer._require_disabled_existing(stale, record)

    def test_api_requests_require_bearer_token_of_minimum_length(self):
        with self.assertRaises(importer.ImportErrorSafe):
            importer.api_headers("too-short")
        with self.assertRaises(importer.ImportErrorSafe):
            importer.api_headers("x" * 32 + " ")

    def test_api_request_headers_include_bearer_token(self):
        headers = importer.api_headers("x" * 32, has_payload=True)
        self.assertEqual(headers["Authorization"], f"Bearer {'x' * 32}")
        self.assertEqual(headers["Content-Type"], "application/json")
        self.assertEqual(headers["Accept"], "application/json")

    def test_redirects_are_not_followed(self):
        self.assertIsNone(importer.NoRedirect().redirect_request(None, None, 302, "Found", {}, "http://example.com"))


if __name__ == "__main__":
    unittest.main()
