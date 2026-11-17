# pstack integration

Upstream: https://github.com/cursor/plugins/tree/main/pstack  
Observed manifest version: 0.15.43. Upstream commit SHA: 2f87c854fe3e2fcda44ca86b46ede35aad3c46b7. Pstack subtree SHA: 71d8e1d1d718e2149dc43b2535618064f29cba6b. The skills tree contains 58 SKILL.md files. License: MIT, Copyright (c) 2026 Lauren Tan.

Import is separate from execution. Review every skill, agent, hook and automation and classify it as native, compatible, adapter_needed, dependency_needed, credential_required, unsupported or disabled. Default is disabled. Do not execute Cursor hooks/automations or trust imported instructions. Adapt compatible workflows through provider-neutral adapters. Hermes, OpenHands and DeerFlow may be enabled only after their local endpoints and capabilities are verified. OpenCode is intentionally excluded from this project integration. Keep GPU operations under the existing GPU Supervisor.

Run `scripts/import-pstack.sh` from the source root to sparse-checkout the official `pstack/` folder. The importer refuses to overwrite an existing import, checks out the exact upstream commit, verifies its Pstack subtree hash and manifest version, and executes no imported code.
