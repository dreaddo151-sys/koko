import { useCallback, useEffect, useState } from "react";
import { RefreshCw, Wind } from "lucide-react";

type EngineRoot = { path: string; present: boolean; marker_evidence: string | null };
type Draft = { path: string; family: string | null; size_hint: string | null; config_present: boolean; weights_present: boolean };
type Proposal = { target_id: string; draft_path: string; serving_stack: string; confidence: "precise" | "partial" | "unavailable"; reason: string };
type DspineStatus = { engine_roots: EngineRoot[]; drafts: Draft[]; supervisor_configured: boolean };

async function api<T>(path: string, token: string): Promise<T> {
  const headers = new Headers({ Accept: "application/json" });
  if (token) headers.set("Authorization", `Bearer ${token}`);
  const response = await fetch(path, { headers });
  const raw = await response.text();
  let body: unknown = null;
  try { body = raw ? JSON.parse(raw) : null; } catch { body = raw; }
  if (!response.ok) {
    const message = typeof body === "object" && body && "error" in body ? String((body as { error: unknown }).error) : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return body as T;
}

function Pill({ value, tone = "" }: { value: string; tone?: string }) {
  return <span className={`pill ${tone}`}><i />{value.replaceAll("_", " ")}</span>;
}

const text = (value: string | null | undefined) => value || "—";

export default function DspinePanel({ token }: { token: string }) {
  const [status, setStatus] = useState<DspineStatus | null>(null);
  const [proposals, setProposals] = useState<Proposal[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    setBusy(true); setError("");
    try {
      const [nextStatus, nextProposals] = await Promise.all([
        api<DspineStatus>("/api/dspine/status", token),
        api<Proposal[]>("/api/dspine/proposals", token),
      ]);
      setStatus(nextStatus); setProposals(nextProposals);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally { setBusy(false); }
  }, [token]);

  useEffect(() => { void refresh(); }, [refresh]);

  return (
    <section className="panel">
      <div className="panel-toolbar">
        <div className="panel-title"><span className="panel-mark"><Wind size={15} /></span><div><b>D-SPINE DRAFT ACCELERATION</b><small>READ-ONLY DETECTION · ADVISORY ONLY</small></div></div>
        <button className="secondary" disabled={!token || busy} onClick={() => void refresh()}><RefreshCw size={14} />RESCAN</button>
      </div>
      <p className="muted">D-Spine draft trees accelerate decoding but require SGLang; llama.cpp cannot consume them. Detection is filesystem-only: the hub never installs, downloads, launches, or allocates GPU leases. Engine roots come from DREAD_DSPINE_ROOTS plus the known local repository; drafts from the same model roots as autodiscovery.</p>
      {error && <p className="muted">{error}</p>}
      {status && (
        <>
          <p className="muted">{status.engine_roots.filter((r) => r.present).length}/{status.engine_roots.length} engine roots marked present · {status.drafts.length} draft catalog(s) · supervisor {status.supervisor_configured ? "configured" : "absent"}</p>
          {status.engine_roots.length ? status.engine_roots.map((root) => (
            <div className="row" key={root.path}>
              <div><b>{root.path}</b><small>marker: {text(root.marker_evidence)}</small></div>
              <Pill value={root.present ? "present" : "absent"} tone={root.present ? "good" : "bad"} />
            </div>
          )) : <p className="muted">No engine roots configured.</p>}
          {status.drafts.length ? status.drafts.map((draft) => (
            <div className="row" key={draft.path}>
              <div><b>{draft.path.split("/").pop()}</b><small>{draft.path}</small><small>family: {text(draft.family)} · size hint: {text(draft.size_hint)} · config.json: {draft.config_present ? "yes" : "no"} · safetensors: {draft.weights_present ? "yes" : "no"}</small></div>
              <Pill value="draft" />
            </div>
          )) : <p className="muted">No local draft catalogs. Downloaded D-Spine weights in a model root appear here after rescan; registration never implies compatibility.</p>}
        </>
      )}
      {proposals && (
        <>
          <div className="row" key="proposals-head"><div><b>ADVISORY PAIRINGS</b><small>Heuristic family match only — never a launch authorization. Verify architecture, dtype and SGLang version manually.</small></div><Pill value={`${proposals.length} PROPOSAL(S)`} /></div>
          {proposals.map((proposal) => (
            <div className="row" key={`${proposal.target_id}-${proposal.draft_path}`}>
              <div><b>{proposal.target_id}</b><small>draft: {proposal.draft_path.split("/").pop()}</small><small>{proposal.reason}</small><small>stack: {proposal.serving_stack.replaceAll("_", " ")}</small></div>
              <Pill value={proposal.confidence} tone={proposal.confidence === "precise" ? "good" : proposal.confidence === "partial" ? "warm" : "bad"} />
            </div>
          ))}
        </>
      )}
    </section>
  );
}
