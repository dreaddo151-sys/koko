import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, Check, ClipboardCheck, Play, RefreshCw, ShieldAlert, X } from "lucide-react";

type RunStatus = "planned" | "awaiting_approval" | "approved" | "applying" | "verified" | "failed" | "rolled_back" | "rejected" | "interrupted";
type ActionStatus = "pending" | "denied" | "applying" | "verified" | "failed" | "skipped" | "interrupted";
type Permission = "pass" | "ask" | "deny";
type RepairRun = { id: number; status: RunStatus; summary: string; safety_boundary: string; created_at: number; updated_at: number; completed_at: number | null; result: string };
type RepairAction = { id: number; run_id: number; position: number; component_id: string; action_id: string; severity: string; decision: Permission; status: ActionStatus; before_state: string; after_state: string; created_at: number; updated_at: number; detail: string };
type AuditRow = { id: number; ts: number; run_id: number; action_id: number | null; component_id: string; action: string; decision: string; actor: string; before_state: string; after_state: string; result: string; detail: string };
type RunDetail = { run: RepairRun; actions: RepairAction[]; audit: AuditRow[]; audit_truncated: boolean };
type RunOutcome = { run_id: number; status: RunStatus; verified_actions: number; failed_actions: number; message: string };
type RepairFinding = { component_id: string; severity: string; condition: string; observed_state: string; evidence: string; recommended_action: string; requires_operator_approval: boolean; auto_apply_eligible: boolean; status: string };
type RepairPlan = { status: string; checked_components: number; findings: RepairFinding[]; summary: string; safety_boundary: string };

async function api<T>(path: string, token: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  headers.set("Accept", "application/json");
  if (token) headers.set("Authorization", `Bearer ${token}`);
  if (init.body) headers.set("Content-Type", "application/json");
  const response = await fetch(path, { ...init, headers });
  const raw = await response.text();
  let body: unknown = null;
  try { body = raw ? JSON.parse(raw) : null; } catch { body = raw; }
  if (!response.ok) {
    const message = typeof body === "object" && body && "error" in body ? String((body as { error: unknown }).error) : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return body as T;
}

function Pill({ value }: { value: string }) {
  const tone = ["online", "ok", "verified", "completed", "approved"].includes(value) ? "good" : ["awaiting", "applying", "pending", "running"].includes(value) ? "warm" : ["failed", "rejected", "interrupted", "denied", "offline"].includes(value) ? "bad" : "";
  return <span className={`pill ${tone}`}><i />{value.replaceAll("_", " ")}</span>;
}

const clock = (value: number | null) => value ? new Date(value * 1000).toLocaleString() : "—";
const text = (value: string | null | undefined) => value || "—";

export default function RepairPanel({ token }: { token: string }) {
  const [plan, setPlan] = useState<RepairPlan | null>(null);
  const [runs, setRuns] = useState<RepairRun[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [detail, setDetail] = useState<RunDetail | null>(null);
  const [outcome, setOutcome] = useState<RunOutcome | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<"diagnostics" | "create" | "runs" | "detail" | "approve" | "reject" | "execute" | null>(null);

  const fail = useCallback((cause: unknown) => {
    const message = cause instanceof Error ? cause.message : String(cause);
    setError(/409|conflict|concurrent/i.test(message) ? `${message} — concurrent state conflict; refresh the run list and try again.` : message);
  }, []);

  const loadRuns = useCallback(async (select?: number) => {
    setBusy("runs"); setError("");
    try {
      const rows = await api<RepairRun[]>("/api/repair/runs?limit=25", token);
      setRuns(rows);
      if (select !== undefined) setSelectedId(select);
    } catch (cause) { fail(cause); } finally { setBusy(null); }
  }, [token, fail]);

  const loadDetail = useCallback(async (id: number) => {
    setBusy("detail"); setError("");
    try { setDetail(await api<RunDetail>(`/api/repair/runs/${id}`, token)); }
    catch (cause) { fail(cause); } finally { setBusy(null); }
  }, [token, fail]);

  useEffect(() => { if (token) void loadRuns(); }, [token, loadRuns]);
  useEffect(() => { if (token && selectedId !== null) void loadDetail(selectedId); }, [token, selectedId, loadDetail]);

  const diagnostics = async () => {
    setBusy("diagnostics"); setError(""); setOutcome(null);
    try { setPlan(await api<RepairPlan>("/api/repair/plan", token)); }
    catch (cause) { fail(cause); } finally { setBusy(null); }
  };

  const createRun = async () => {
    if (!plan) return;
    setBusy("create"); setError(""); setOutcome(null);
    try {
      const run = await api<RepairRun>("/api/repair/runs", token, { method: "POST", body: JSON.stringify(plan) });
      setSelectedId(run.id); setDetail(null);
      await loadRuns(run.id);
    } catch (cause) { fail(cause); } finally { setBusy(null); }
  };

  const mutate = async (kind: "approve" | "reject") => {
    if (selectedId === null || !window.confirm(`${kind === "approve" ? "Approve" : "Reject"} repair run ${selectedId}?`)) return;
    setBusy(kind); setError(""); setOutcome(null);
    try {
      const run = await api<RepairRun>(`/api/repair/runs/${selectedId}/${kind}`, token, { method: "POST" });
      setRuns(current => current.map(item => item.id === run.id ? run : item));
      setDetail(current => current?.run.id === run.id ? { ...current, run } : current);
      await loadDetail(run.id);
    } catch (cause) { fail(cause); } finally { setBusy(null); }
  };

  const execute = async () => {
    if (selectedId === null || !window.confirm(`Execute repair run ${selectedId}? The v1 runner is simulated; no host mutation will occur.`)) return;
    setBusy("execute"); setError(""); setOutcome(null);
    try {
      const result = await api<RunOutcome>(`/api/repair/runs/${selectedId}/execute`, token, { method: "POST" });
      setOutcome(result);
      await Promise.all([loadDetail(result.run_id), loadRuns(result.run_id)]);
    } catch (cause) { fail(cause); } finally { setBusy(null); }
  };

  const run = detail?.run ?? runs.find(item => item.id === selectedId) ?? null;
  const outcomeTone = outcome?.status === "verified" ? "good" : outcome?.status === "failed" || outcome?.status === "interrupted" ? "bad" : "warm";

  return <div className="command-grid repair-panel">
    <style>{`.repair-panel .card{min-width:0}.repair-panel .data-table{width:100%;border-collapse:collapse;font-size:12px}.repair-panel .data-table th,.repair-panel .data-table td{text-align:left;vertical-align:top;padding:6px 8px;border-bottom:1px solid rgba(255,255,255,.08);overflow-wrap:anywhere}.repair-panel .data-table th{opacity:.65;font-weight:600}.repair-panel .repair-scroll{overflow:auto;max-width:100%}.repair-panel .repair-banner{padding:10px 12px;margin:8px 0;border:1px solid currentColor;border-radius:6px}.repair-panel .repair-banner.bad{color:#ff7b7b}.repair-panel .repair-banner.good{color:#72d99a}.repair-panel .repair-banner.warm{color:#e8bd68}.repair-panel .repair-copy{opacity:.75;line-height:1.5}.repair-panel .repair-selected{background:rgba(255,255,255,.06)}.repair-panel .repair-click{cursor:pointer}`}</style>
    <section className="card">
      <h2><ShieldAlert size={18} /> Self-repair</h2>
      <p className="repair-copy">The v1 runner is SIMULATED (no host mutation); interrupted runs are never auto-resumed — a new run must be created; every action requires an explicit operator approve in the audit trail.</p>
      <div className="button-row">
        <button disabled={busy !== null} onClick={() => void diagnostics()}><ClipboardCheck size={14} />{busy === "diagnostics" ? "Running…" : "Run diagnostics"}</button>
        <button disabled={!plan || busy !== null} onClick={() => void createRun()}><Play size={14} />{busy === "create" ? "Creating…" : "Create repair run"}</button>
        <button disabled={busy !== null} onClick={() => void loadRuns()}><RefreshCw size={14} />{busy === "runs" ? "Refreshing…" : "Refresh runs"}</button>
      </div>
      {error && <div className="repair-banner bad" role="alert"><AlertTriangle size={14} /> {error}</div>}
      {outcome && <div className={`repair-banner ${outcomeTone}`} role="status"><strong>Run {outcome.run_id}: {outcome.status.replaceAll("_", " ")}</strong> — {outcome.message} Verified: {outcome.verified_actions}; failed: {outcome.failed_actions}.</div>}
    </section>

    {plan && <section className="card">
      <h3>Diagnostics plan <Pill value={plan.status} /></h3>
      <p>{plan.summary} Checked components: {plan.checked_components}.</p>
      <p className="repair-copy"><strong>Safety boundary:</strong> {plan.safety_boundary}</p>
      <div className="repair-scroll"><table className="data-table"><thead><tr><th>Component</th><th>Severity</th><th>Condition</th><th>Observed state</th><th>Recommended action</th><th>Operator approval</th><th>Auto-apply eligible</th><th>Status</th></tr></thead><tbody>
        {plan.findings.map((finding, index) => <tr key={`${finding.component_id}-${index}`}><td>{finding.component_id}</td><td>{finding.severity}</td><td>{finding.condition}</td><td>{finding.observed_state}</td><td>{finding.recommended_action}</td><td>{finding.requires_operator_approval ? "required" : "no"}</td><td>{finding.auto_apply_eligible ? "yes" : "no"}</td><td><Pill value={finding.status} /></td></tr>)}
        {!plan.findings.length && <tr><td colSpan={8}>No findings.</td></tr>}
      </tbody></table></div>
    </section>}

    <section className="card">
      <h3>Repair runs</h3>
      <div className="repair-scroll"><table className="data-table"><thead><tr><th>ID</th><th>Status</th><th>Summary</th><th>Result</th><th>Updated</th></tr></thead><tbody>
        {runs.map(item => <tr key={item.id} className={`repair-click ${item.id === selectedId ? "repair-selected" : ""}`} onClick={() => { setSelectedId(item.id); setOutcome(null); }}><td>#{item.id}</td><td><Pill value={item.status} /></td><td>{text(item.summary)}</td><td>{text(item.result)}</td><td>{clock(item.updated_at)}</td></tr>)}
        {!runs.length && <tr><td colSpan={5}>{busy === "runs" ? "Loading runs…" : "No repair runs."}</td></tr>}
      </tbody></table></div>
    </section>

    {run && <section className="card">
      <h3>Run #{run.id} <Pill value={run.status} /></h3>
      <p>{text(run.summary)}</p>
      <p><strong>Result:</strong> {text(run.result)}</p>
      <p className="repair-copy"><strong>Safety boundary:</strong> {run.safety_boundary}<br />Created {clock(run.created_at)} · Updated {clock(run.updated_at)} · Completed {clock(run.completed_at)}</p>
      <div className="button-row">
        {(run.status === "awaiting_approval") && <button disabled={busy !== null} onClick={() => void mutate("approve")}><Check size={14} />{busy === "approve" ? "Approving…" : "Approve"}</button>}
        {(run.status === "planned" || run.status === "awaiting_approval") && <button disabled={busy !== null} onClick={() => void mutate("reject")}><X size={14} />{busy === "reject" ? "Rejecting…" : "Reject"}</button>}
        {run.status === "approved" && <button disabled={busy !== null} onClick={() => void execute()}><Play size={14} />{busy === "execute" ? "Executing…" : "Execute"}</button>}
      </div>
      {busy === "detail" && <p>Loading run detail…</p>}
      {detail?.run.id === run.id && <>
        <h4>Actions</h4>
        <div className="repair-scroll"><table className="data-table"><thead><tr><th>Position</th><th>Component</th><th>Action ID</th><th>Severity</th><th>Decision</th><th>Status</th><th>Before state</th><th>After state</th><th>Detail</th></tr></thead><tbody>
          {detail.actions.map(action => <tr key={action.id}><td>{action.position}</td><td>{action.component_id}</td><td>{action.action_id}</td><td>{action.severity}</td><td>{action.decision}</td><td><Pill value={action.status} /></td><td>{text(action.before_state)}</td><td>{text(action.after_state)}</td><td>{text(action.detail)}</td></tr>)}
          {!detail.actions.length && <tr><td colSpan={9}>No executable actions.</td></tr>}
        </tbody></table></div>
        <h4>Audit trail {detail.audit_truncated && <span>(truncated)</span>}</h4>
        <div className="repair-scroll"><table className="data-table"><thead><tr><th>Timestamp</th><th>Actor</th><th>Action</th><th>Decision</th><th>Result</th><th>Detail</th><th>Component</th><th>Action ID</th></tr></thead><tbody>
          {detail.audit.map(row => <tr key={row.id}><td>{clock(row.ts)}</td><td>{row.actor}</td><td>{row.action}</td><td>{row.decision}</td><td>{text(row.result)}</td><td>{text(row.detail)}</td><td>{text(row.component_id)}</td><td>{row.action_id ?? "—"}</td></tr>)}
          {!detail.audit.length && <tr><td colSpan={8}>No audit entries.</td></tr>}
        </tbody></table></div>
      </>}
    </section>}
  </div>;
}
