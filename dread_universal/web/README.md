# DREAD Universal — Work Monitor (web/)
Static UI for the canonical dread-api route set (verified live against sandbox @413698b, 2026-11-22).
- Auth: bearer token in sessionStorage ONLY (security rule: never localStorage).
- Data endpoints: /health, /api/tasks (+approve/cancel with reason), /api/registry,
  /api/services, /api/gpu/telemetry (graceful 503), /api/usage/tokens,
  /api/capabilities/discover, /api/extensions/discover.
- /v1/* mock handlers are NOT used by this UI (CI fixtures, not runtime data).
- All API-derived strings are HTML-escaped (esc()).
- Serving: static only for now; binary static hosting arrives with packaging block (tower-http decision logged).
