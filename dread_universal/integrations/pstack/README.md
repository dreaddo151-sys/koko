# pstack integration

Upstream: https://github.com/cursor/plugins/tree/main/pstack
Observed manifest version: 0.15.43. Upstream tree SHA: 2f87c854fe3e2fcda44ca86b46ede35aad3c46b7. The skills tree contains 58 SKILL.md files. License: MIT, Copyright (c) 2026 Lauren Tan.

Import is separate from execution. Review every skill, agent, hook and automation and classify it as native, compatible, adapter_needed, dependency_needed, credential_required, unsupported or disabled. Default is disabled. Do not execute Cursor hooks/automations or trust imported instructions. Adapt compatible workflows to Hermes, OpenHands and OpenCode. Keep GPU operations under the existing GPU Supervisor.

Run `scripts/import-pstack.sh` from the source root to sparse-checkout the official `pstack/` folder. The importer refuses to overwrite an existing import and executes no imported code.