// DREAD Universal — Work Monitor (canonical API, PR#16 route set)
// Security rules: bearer token lives ONLY in sessionStorage (never localStorage);
// every API-derived string is HTML-escaped before insertion.
const SKEY = "dread_api_token";
const tokenInput = document.getElementById("api-token");
tokenInput.value = sessionStorage.getItem(SKEY) || "";
tokenInput.addEventListener("change", () => {
  const v = tokenInput.value.trim();
  if (v) sessionStorage.setItem(SKEY, v); else sessionStorage.removeItem(SKEY);
  refreshAll();
});

const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) =>
  ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

async function api(path, opts = {}) {
  const headers = { "Content-Type": "application/json", ...(opts.headers || {}) };
  const token = sessionStorage.getItem(SKEY);
  if (token) headers["Authorization"] = `Bearer ${token}`;
  const res = await fetch(path, { ...opts, headers });
  if (!res.ok) throw new Error(`${res.status} ${await res.text()}`);
  return res.status === 204 ? null : res.json();
}
const items = (v) => Array.isArray(v) ? v : (v?.items ?? []);

function renderTelemetry(t) {
  const cpu = t.cpu ?? t.system ?? {};
  document.getElementById("cpu-ram").textContent =
    `CPU: ${esc(cpu.percent ?? "--")}% | RAM: ${esc(cpu.ram_used ?? cpu.memory_used ?? "--")} / ${esc(cpu.ram_total ?? cpu.memory_total ?? "--")} | load: ${esc(cpu.load ?? "--")}`;
  const gpus = t.gpus ?? t.devices ?? [];
  document.getElementById("gpu-list").innerHTML = gpus.map((g) => `<div class="item">
      <strong>GPU ${esc(g.index)}: ${esc(g.name)}</strong><br/>
      VRAM: ${esc(g.memory_used_mb ?? g.vram_used_mb ?? "--")} / ${esc(g.memory_total_mb ?? g.vram_total_mb ?? "--")} MiB
      | Util: ${esc(g.utilization ?? g.utilization_percent ?? "--")}%
      | ${esc(g.temperature_c ?? "--")}°C | ${esc(g.power_watts ?? g.power_draw_watts ?? "--")}W
    </div>`).join("") || `<div class="item">No GPU telemetry (Supervisor bridge not configured for this instance)</div>`;
}

async function refreshAll() {
  try {
    const health = await api("/health");
    document.getElementById("hub-status").textContent = `ONLINE ${esc(health.version ?? "")}`;
    document.getElementById("hub-status").className = "badge ok";
  } catch { document.getElementById("hub-status").textContent = "OFFLINE / AUTH"; return; }

  // Independent panels: one failing upstream must not blank the rest.
  try { renderTelemetry(await api("/api/gpu/telemetry")); }
  catch (e) { document.getElementById("gpu-list").innerHTML = `<div class="item">${esc(e.message.split(" ")[0])} — telemetry unavailable</div>`; }

  try {
    const [tasks, registry, services, usage, providers] = await Promise.all([
      api("/api/tasks"), api("/api/registry"), api("/api/services"),
      api("/api/usage/tokens").catch(() => null),
      api("/api/providers").catch(() => null),
    ]);
    document.getElementById("provider-list").innerHTML = providers
      ? `Providers (${esc(providers.count ?? items(providers.providers).length)}): ` +
        items(providers.providers).map((p) =>
          `<span class="item-inline">${esc(p.name)} [${esc(String(p.kind || "").replace("provider.", ""))}]${p.metadata?.dropin_enabled === false ? " (disabled)" : ""}</span>`).join(" ") || "none registered"
      : "";
    document.getElementById("token-usage").textContent = usage
      ? `Tokens — prompt: ${esc(usage.prompt_tokens ?? usage.total?.prompt ?? "--")} | completion: ${esc(usage.completion_tokens ?? usage.total?.completion ?? "--")} | total: ${esc(usage.total_tokens ?? usage.total?.total ?? "--")}`
      : "Token usage: no data";
    document.getElementById("inventory").innerHTML =
      `<strong>Registry (${registry.count ?? items(registry).length})</strong>` +
      items(registry).map((r) => `<div class="item">${esc(r.name)} [${esc(r.kind)}] → ${esc(r.endpoint)}</div>`).join("") +
      `<br/><strong>Services (${items(services).length})</strong>` +
      items(services).map((s) => `<div class="item">${esc(s.name ?? s.id)} [${esc(s.status ?? "?")}] ${esc(s.endpoint ?? "")}</div>`).join("");
    const tl = document.getElementById("job-list");
    const list = items(tasks).slice(0, 12);
    tl.innerHTML = list.map((j) => `<div class="item" data-id="${esc(j.id)}">
        <strong>${esc(j.title)}</strong> <code>${esc(j.task_type)}</code> [${esc(j.status)}]
        ${esc((j.id || "").slice(0, 8))}
        ${j.status === "pending_approval"
          ? `<button class="approve">Approve</button><button class="cancel">Cancel</button>` : ""}
      </div>`).join("") || `<div class="item">No tasks yet.</div>`;
    tl.querySelectorAll(".approve").forEach((b) => b.onclick = (ev) => taskAction(ev, "approve"));
    tl.querySelectorAll(".cancel").forEach((b) => b.onclick = (ev) => taskAction(ev, "cancel"));
  } catch (e) { console.error(e); }
}

async function taskAction(ev, action) {
  const id = ev.target.closest(".item").dataset.id;
  try { await api(`/api/tasks/${id}/${action}`, { method: "POST", body: JSON.stringify({ reason: "approved from work monitor" }) }); } catch (e) { alert(e.message); }
  refreshAll();
}

document.getElementById("btn-discover").addEventListener("click", async () => {
  await Promise.allSettled([
    api("/api/capabilities/discover", { method: "POST" }),
    api("/api/extensions/discover", { method: "POST" }),
  ]);
  refreshAll();
});

async function hubAction(btn, fn) {
  const status = document.getElementById("sync-status");
  const old = btn.textContent;
  btn.disabled = true; btn.textContent = "Working…"; status.textContent = "";
  try { await fn(); status.textContent = "done " + new Date().toLocaleTimeString(); }
  catch (e) { status.textContent = e.message.slice(0, 120); }
  btn.disabled = false; btn.textContent = old;
  refreshAll();
}
document.getElementById("btn-providers-sync").addEventListener("click",
  (ev) => hubAction(ev.target, () => api("/api/providers/sync", { method: "POST" })));
document.getElementById("btn-models-autoconfig").addEventListener("click",
  (ev) => hubAction(ev.target, () => api("/api/models/autoconfigure", { method: "POST" })));

document.getElementById("task-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const title = document.getElementById("task-title").value.trim();
  const task_type = document.getElementById("task-type").value.trim() || "chat.prompt";
  let payload = {};
  try { payload = JSON.parse(document.getElementById("task-payload").value || "{}"); }
  catch { alert("Payload must be valid JSON"); return; }
  try {
    await api("/api/tasks", { method: "POST", body: JSON.stringify({ title, task_type, payload }) });
    document.getElementById("task-title").value = "";
  } catch (e) { alert(e.message); }
  refreshAll();
});

refreshAll();
setInterval(refreshAll, 4000);
