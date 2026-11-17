# dread-skills

Provider-neutral, local-only discovery and bounded loading of Markdown skills.

- Scans only immediate `skills/<id>/SKILL.md` entries.
- Rejects symlinks, path traversal, non-UTF-8 files, oversized files and oversized catalogs.
- Does not execute Markdown, scripts, hooks, MCP tools, shell commands or network requests.
- Returns content as untrusted context; callers must keep system policy and tool permissions separate.
- The FNV-1a fingerprint is only a change detector, not a cryptographic signature or provenance proof.

Set `DREAD_SKILLS_ROOT` to the root directory containing the `skills/` folder.
