# dread-skills

Provider-neutral, local-only discovery and bounded loading of Markdown skills and agent prompt profiles.

- Scans `skills/<name>/SKILL.md` and optional `agents/<name>.md` entries.
- Rejects symlinks, path traversal, non-UTF-8 files, oversized files and oversized catalogs.
- Does not execute Markdown, scripts, hooks, MCP tools, shell commands or network requests.
- A Markdown agent profile is not a live AgentAdapter process; it is loaded only as untrusted model context.
- Imported content is untrusted and cannot grant permissions or override DREAD policy.
- The FNV-1a fingerprint is only a local change detector, not a cryptographic signature or provenance proof.

Set `DREAD_SKILLS_ROOT` to the root directory containing `skills/` and optionally `agents/`.
