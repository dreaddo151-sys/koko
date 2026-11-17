# Pstack reconciliation — run #81

Date: 2026-11-17  
Repository: `dreaddo151-sys/koko`  
Branch: `feature/agent-capability-foundation-2026-11-17`  
Verified source artifact: run `44017474488`, artifact `14298824294`  
Artifact SHA-256: `dfeaffa4fcac30f2a6d7353e9c2b4b138cf88573f9244b443ca00075429419e6`

## Finding

Pstack is **not missing** from the run #81 source bundle. Two distinct directories are present:

- `integrations/pstack/upstream/`: the canonical imported source, pinned to Pstack `0.15.43`.
- `integrations/pstack-upstream/`: a retained older bundled snapshot whose manifest reports `0.15.42`.

The old snapshot must not be relabelled or silently substituted for the pinned source. It is retained as historical/reference material. The importer targets `integrations/pstack/upstream`, not the older sibling directory.

## Independent verification performed on the downloaded run #81 artifact

- Catalog version: `0.15.43`.
- Pinned commit: `2f87c854fe3e2fcda44ca86b46ede35aad3c46b7`.
- Pstack subtree: `71d8e1d1d718e2149dc43b2535618064f29cba6b`.
- Imported `plugin.json` version: `0.15.43`.
- Import provenance marker commit/tree/version: exact match to catalog.
- Catalog inventory: 58 skills and 2 agents.
- Provenance marker: 60 per-file SHA-256 entries.
- Recomputed SHA-256 for every catalogued skill and agent: PASS.
- `python3 scripts/test_import_pstack_capabilities.py`: PASS, 9 tests.
- `python3 scripts/import_pstack_capabilities.py --root <source-root>`: PASS, dry-run validates 58 skills and 2 agents; all start disabled.
- No imported skill, agent, hook, or script was executed. Live API `--apply` was not run.

## Correct interpretation

The `0.15.42` manifest is not evidence that the Pstack integration was lost or that the active importer is using the wrong version. The exact `0.15.43` import exists at the path consumed by the importer and its catalogued files match the provenance hashes.

The old sibling snapshot is potentially confusing. Keep it unchanged until a separately reviewed cleanup decision; do not delete it as part of this reconciliation.

## Remaining Pstack gates

1. Run the live API `--apply` only against the intended loopback Rust API with `DREAD_API_TOKEN` configured after confirming it is the correct instance and the capability registry is empty or safely reconciled.
2. Verify persisted records by reading them back and confirming every imported record is disabled with matching source revision.
3. Keep compatibility review and adapter implementation separate from ingestion. No capability is activated by this import.
4. Add an automated CI assertion that the catalog, active import manifest, provenance marker, inventory, and per-file hashes agree, and that the retained legacy snapshot is never used as the import source.

## Tooling limitation during this review

The review container has no `rustc`/Cargo toolchain installed, so local Rust checks could not be rerun here. The run #81 GitHub Actions result remains the Rust build/test evidence; Python importer checks and the full Pstack source/hash reconciliation were rerun locally against the downloaded artifact.
