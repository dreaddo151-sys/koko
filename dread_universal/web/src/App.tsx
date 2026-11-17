import { useCallback, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { Activity, AlertCircle, Bot, Check, ChevronRight, Cpu, Gauge, KeyRound, Layers3, LockKeyhole, LogOut, MessageSquare, Play, Plus, Radio, RefreshCw, Search, Server, ShieldCheck, TerminalSquare, X, Zap, Workflow } from "lucide-react";

type Model = { id: string };
type Task = { id: string; title: string; task_type: string; status: string; created_at: number };
type Agent = { id: string; name: string; status: string; adapter?: string };
type Event = { sequence: number; event_type: string; actor: string; created_at: number };
type Result = { result?: unknown; error?: string | null; attempts?: number; elapsed_ms?: number };
type Capability = { id: string; name: string; kind: string; description: string; version?: string | null; license?: string | null; compatibility: string; dependencies: string[]; adapter?: string | null; status: string; provenance: { source_name: string; source_version?: string | null } };
type SkillPreview = { id: string; name: string; description: string; content: string; trust: string; execution: string };
type Service = { id: string; name: string; role: string; status: string; http_status?: number | null; latency_ms?: number; summary?: string | null; detail?: string };
type Page = "overview" | "tasks" | "agents" | "capabilities" | "models" | "gpu";

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
    const message = typeof body === "object" && body && "error" in body ? String((body as {error: unknown}).error) : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return body as T;
}
const clock = (value?: number) => value ? new Date(value * 1000).toLocaleTimeString([], {hour:"2-digit",minute:"2-digit"}) : "—";

function Pill({value}:{value:string}) {
  const s = value.toLowerCase();
  const tone = ["online","ok","completed","approved","ready"].includes(s) ? "good" : ["running","queued","pending_approval"].includes(s) ? "warm" : ["failed","offline","cancelled","degraded","locked"].includes(s) ? "bad" : "";
  return <span className={`pill ${tone}`}><i />{value.replaceAll("_"," ")}</span>;
}
function App() {
  const [token,setToken]=useState(""); const [draft,setDraft]=useState(""); const [show,setShow]=useState(false);
  const [connected,setConnected]=useState(false); const [supervisorOnline,setSupervisorConfigured]=useState(false); const [error,setError]=useState(""); const [busy,setBusy]=useState(false);
  const [models,setModels]=useState<Model[]>([]); const [model,setModel]=useState("");
  const [tasks,setTasks]=useState<Task[]>([]); const [agents,setAgents]=useState<Agent[]>([]); const [capabilities,setCapabilities]=useState<Capability[]>([]); const [services,setServices]=useState<Service[]>([]);
  const [skillPreview,setSkillPreview]=useState<SkillPreview|null>(null); const [notice,setNotice]=useState(""); const [selectedSkills,setSelectedSkills]=useState<string[]>([]);
  const [prompt,setPrompt]=useState(""); const [active,setActive]=useState("");
  const [events,setEvents]=useState<Event[]>([]); const [result,setResult]=useState<Result|null>(null);
  const [page,setPage]=useState<Page>("overview"); const [lastRefresh,setLastRefresh]=useState<number|null>(null);

  const refresh=useCallback(async()=>{
    if(!token){setConnected(false);return;}
    try{
      const [health,gpuHealth,ml,tl,al,cl,sv]=await Promise.all([
        api<{status:string}>("/health",""), api<{supervisor_online?:boolean}>("/api/gpu/health",token), api<{data?:Model[]}>("/api/gpu/models",token),
        api<{items?:Task[]}>("/api/tasks",token), api<{items?:Agent[]} >("/api/agents",token), api<{items?:Capability[]}>("/api/capabilities",token), api<Service[]>("/api/services",token)
      ]);
      setConnected(health.status==="ok"); setSupervisorConfigured(Boolean(gpuHealth.supervisor_online)); setModels(ml.data??[]); setTasks(tl.items??[]); setAgents(al.items??[]); setCapabilities(cl.items??[]); setServices(sv??[]); setError(""); setLastRefresh(Date.now());
    }catch(e){setConnected(false);setError(e instanceof Error?e.message:"API request failed");}
  },[token]);
  useEffect(()=>{void refresh();if(!token)return;const t=window.setInterval(()=>void refresh(),5000);return()=>window.clearInterval(t);},[refresh,token]);
  useEffect(()=>{if(models.length&&!models.some(x=>x.id===model))setModel(models[0].id);},[models,model]);
  useEffect(()=>{
    if(!token||!active){setEvents([]);setResult(null);return;}
    let dead=false;
    const load=async()=>{try{
      const [ev,res]=await Promise.all([api<{items?:Event[]}>(`/api/tasks/${active}/events`,token),api<Result>(`/api/tasks/${active}/result`,token).catch(()=>null)]);
      if(!dead){setEvents(ev.items??[]);setResult(res);}
    }catch{if(!dead)setEvents([]);}};
    void load();const t=window.setInterval(()=>void load(),2500);
    return()=>{dead=true;window.clearInterval(t);};
  },[active,token]);
  const stats=useMemo(()=>({active:tasks.filter(x=>["queued","running"].includes(x.status)).length,pending:tasks.filter(x=>x.status==="pending_approval").length,done:tasks.filter(x=>x.status==="completed").length}),[tasks]);
  const onlineServices=services.filter(x=>x.status==="ONLINE").length;
  const comfy=services.find(x=>x.id==="comfyui");
  const connect=()=>{if(!draft.trim()){setError("Enter DREAD_API_TOKEN.");return;}setToken(draft.trim());setDraft("");};
  const create=async(type:"gpu_chat"|"service_discovery")=>{
    if(!token)return;if(type==="gpu_chat"&&!supervisorOnline){setError("GPU inference is locked until the Python GPU Supervisor bridge is online and authenticated.");return;}if(type==="gpu_chat"&&(!model||!prompt.trim())){setError("Select a model and enter a prompt.");return;}
    setBusy(true);setError("");
    try{
      const payload=type==="gpu_chat"?{model,messages:[{role:"user",content:prompt.trim()}],max_tokens:1024,skill_ids:selectedSkills}:{};
      const out=await api<{item:Task}>("/api/tasks",token,{method:"POST",body:JSON.stringify({title:type==="gpu_chat"?`Chat · ${model}`:"Local service discovery",task_type:type,payload})});
      setTasks(x=>[out.item,...x]);setActive(out.item.id);setPage("tasks");if(type==="gpu_chat")setPrompt("");
    }catch(e){setError(e instanceof Error?e.message:"Task creation failed");}finally{setBusy(false);}
  };
  const action=async(task:Task,verb:"approve"|"run"|"cancel")=>{
    setBusy(true);setError("");
    try{await api(`/api/tasks/${task.id}/${verb}`,token,{method:"POST",body:verb==="approve"?JSON.stringify({reason:"Approved in DREAD Universal Console"}):undefined});await refresh();setActive(task.id);}
    catch(e){setError(e instanceof Error?e.message:`Task ${verb} failed`);}finally{setBusy(false);}
  };
  const discoverSkills=async()=>{
    setBusy(true);setError("");setNotice("");
    try{
      const found=await api<{count:number}>("/api/capabilities/discover",token);
      const imported=await api<{discovered:number;imported:number;already_present:number}>("/api/capabilities/import",token,{method:"POST"});
      await refresh();
      setNotice(`Pstack scan: ${found.count} found · ${imported.imported} imported · ${imported.already_present} already present. All imports remain disabled.`);
    }catch(e){setError(e instanceof Error?e.message:"Pstack discovery failed");}
    finally{setBusy(false);}
  };
  const previewSkill=async(item:Capability)=>{
    setBusy(true);setError("");
    try{const preview=await api<SkillPreview>(`/api/capabilities/${encodeURIComponent(item.id)}/preview`,token);setSkillPreview(preview);}
    catch(e){setError(e instanceof Error?e.message:"Skill preview failed");}
    finally{setBusy(false);}
  };
  const enableSkill=async()=>{
    if(!skillPreview)return;setBusy(true);setError("");
    try{
      await api(`/api/capabilities/${encodeURIComponent(skillPreview.id)}/enable`,token,{method:"POST",body:JSON.stringify({reason:"Reviewed the displayed Markdown source in DREAD Universal Console; enable as untrusted model context only."})});
      setNotice(`Enabled ${skillPreview.name} as untrusted model context. No code execution is permitted.`);
      setSkillPreview(null);await refresh();
    }catch(e){setError(e instanceof Error?e.message:"Skill approval failed");}
    finally{setBusy(false);}
  };
  const disableSkill=async(item:Capability)=>{
    setBusy(true);setError("");
    try{await api(`/api/capabilities/${encodeURIComponent(item.id)}/disable`,token,{method:"POST"});setNotice(`Disabled ${item.name}.`);setSkillPreview(null);await refresh();}
    catch(e){setError(e instanceof Error?e.message:"Skill disable failed");}
    finally{setBusy(false);}
  };
  const disconnect=()=>{setToken("");setDraft("");setPage("overview");setActive("");setEvents([]);setResult(null);setTasks([]);setAgents([]);setCapabilities([]);setModels([]);setServices([]);setModel("");setConnected(false);setSupervisorConfigured(false);};
  const activeTask=tasks.find(x=>x.id===active);
  const nav:[Page,string,ReactNode,string][]=[
    ["overview","Command deck",<Activity/>,"01"],["gpu","GPU telemetry",<Gauge/>,"02"],["models","Model arsenal",<Layers3/>,"03"],["agents","Agent network",<Bot/>,"04"],["tasks","Mission queue",<Workflow/>,"05"],["capabilities","Pstack registry",<ShieldCheck/>,"06"]
  ];
  return <div className="shell">
    <aside className="sidebar">
      <div className="brand"><div className="brand-icon"><TerminalSquare size={22}/></div><div><b>DREAD<span> //</span></b><small>UNIVERSAL CONTROL SYSTEM</small></div></div>
      <div className="side-label">OPERATIONS</div>
      {nav.map(([id,label,icon,number])=><button key={id} className={page===id?"nav active":"nav"} onClick={()=>setPage(id)}><span className="nav-icon">{icon}</span><span className="nav-copy">{label}</span><small>{number}</small></button>)}
      <div className="sidebar-bottom"><div className="safety"><ShieldCheck size={17}/><div><b>SECURITY ENVELOPE</b><small>Loopback only · approval gated<br/>Supervisor owns GPU leases</small></div></div><div className="side-foot"><span className="pulse-dot"/>LOCAL-FIRST / NO REMOTE EXECUTION</div></div>
    </aside>
    <main>
      <header className="topbar"><div className="crumb">DREAD SYSTEM <ChevronRight size={13}/> <b>{page.replaceAll("_"," ").toUpperCase()}</b></div><div className="topright"><span className={connected?"online":"offline"}><i/>{connected?"CONTROL LINK ACTIVE":token?"API LINK LOST":"SYSTEM LOCKED"}</span><span className="refresh-time">{lastRefresh?new Date(lastRefresh).toLocaleTimeString([],{hour:"2-digit",minute:"2-digit",second:"2-digit"}):"NO TELEMETRY"}</span><button className="icon" onClick={()=>void refresh()} title="Refresh telemetry"><RefreshCw size={15}/></button>{token&&<button className="icon" title="Disconnect and clear token" onClick={disconnect}><LogOut size={15}/></button>}</div></header>
      <div className="content">
        <div className="eyebrow"><span className="pulse-dot"/> LOCAL AI OPERATIONS / DUAL GPU COMMAND ENVIRONMENT <span className="eyebrow-line"/></div>
        <div className="heading"><div><h1>{page==="overview"?"Command deck":page==="tasks"?"Mission control":page==="agents"?"Agent network":page==="capabilities"?"Pstack capability registry":page==="models"?"Model arsenal":"GPU telemetry"}</h1><p>Observe, coordinate and operate your local AI stack without crossing its safety boundaries.</p></div><div className="workstation"><span className="workstation-mark"><Cpu size={15}/></span><div><b>DUAL RTX WORKSTATION</b><small>LOCAL NODE / LOOPBACK</small></div><Pill value={connected?"LINKED":"LOCKED"}/></div></div>
        {!token&&<div className="connect"><LockKeyhole size={21}/><div><b>AUTHENTICATE COMMAND LINK</b><small>Bearer token remains in page memory only. Never persisted to browser storage.</small></div><div className="token"><KeyRound size={15}/><input type={show?"text":"password"} value={draft} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>e.key==="Enter"&&connect()} placeholder="DREAD API ACCESS TOKEN"/><button className="icon" onClick={()=>setShow(!show)}><Search size={14}/></button><button className="primary" onClick={connect}>ESTABLISH LINK</button></div></div>}
        {error&&<div className="error"><AlertCircle size={16}/>{error}<button onClick={()=>setError("")}><X size={14}/></button></div>}
        <div className="metrics">
          <Metric icon={<Cpu/>} label="GPU SUPERVISOR" value={supervisorOnline?"READY":"LOCKED"} detail="PYTHON LEASE BRIDGE / :11502" tone={supervisorOnline?"cyan":"amber"}/>
          <Metric icon={<Activity/>} label="ACTIVE MISSIONS" value={String(stats.active).padStart(2,"0")} detail={`${stats.pending} AWAITING OPERATOR APPROVAL`} tone="violet"/>
          <Metric icon={<Layers3/>} label="LOCAL MODEL POOL" value={String(models.length).padStart(2,"0")} detail="DISCOVERED VIA GPU GATEWAY" tone="cyan"/>
          <Metric icon={<Radio/>} label="SERVICE UPLINKS" value={String(onlineServices).padStart(2,"0")} detail={`${services.length} CONFIGURED / READ-ONLY PROBES`} tone="green"/>
        </div>
        {page==="overview"&&<div className="command-grid">
          <section className="panel inference-panel"><PanelTitle icon={<Zap/>} title="INFERENCE COMMAND" meta="APPROVAL REQUIRED"/><div className="panel-intro">Submit a local task. Execution remains gated until an operator approves it.</div>
            <label>MODEL ROUTE</label><select value={model} onChange={e=>setModel(e.target.value)} disabled={!token||!models.length}><option value="">Connect to discover local models</option>{models.map(x=><option key={x.id}>{x.id}</option>)}</select>
            <label>MISSION PROMPT</label><textarea rows={5} value={prompt} onChange={e=>setPrompt(e.target.value)} placeholder="Define the objective for your local model..." disabled={!token}/>
            <div className="skill-picker"><label>APPROVED SKILL CONTEXT / OPTIONAL</label>{capabilities.filter(x=>x.kind==="skill"&&x.status==="enabled").length?capabilities.filter(x=>x.kind==="skill"&&x.status==="enabled").map(skill=><label className="skill-choice" key={skill.id}><input type="checkbox" checked={selectedSkills.includes(skill.id)} onChange={e=>setSelectedSkills(current=>e.target.checked?[...current,skill.id]:current.filter(id=>id!==skill.id))}/><span>{skill.name}<small>{skill.id}</small></span></label>):<small>No enabled skills. Import, preview and approve them in the Pstack registry.</small>}</div>
            <div className="compose-foot"><small>TEXT ONLY / NON-STREAMING / RESULT CAP 1 MiB</small><button className="primary" disabled={!token||!supervisorOnline||busy||!model||!prompt.trim()} onClick={()=>void create("gpu_chat")}><Plus size={14}/>QUEUE INFERENCE</button></div>
            <div className="approval"><ShieldCheck size={14}/>{supervisorOnline?"Task creation does not execute inference. Operator approval is required.":"GPU inference locked — authenticated Python Supervisor bridge unavailable."}</div>
          </section>
          <section className="panel gpu-strip"><PanelTitle icon={<Gauge/>} title="DUAL GPU ARRAY" meta="TELEMETRY SOURCE REQUIRED"/><div className="gpu-pair"><GpuCard index="GPU 0" title="RTX 3090" locked={!supervisorOnline}/><GpuCard index="GPU 1" title="RTX 3090" locked={!supervisorOnline}/></div><div className="telemetry-note"><Activity size={13}/> Utilization, VRAM, temperature and power readings are not exposed by the current API. No synthetic values are displayed.</div><button className="link" onClick={()=>setPage("gpu")}>OPEN GPU TELEMETRY <ChevronRight size={14}/></button></section>
          <section className="panel system-panel"><PanelTitle icon={<Workflow/>} title="SYSTEM SYNAPSE" meta="LIVE SERVICE MAP"/><div className="synapse"><div className="syn-node hub-node"><span className="node-core"><Activity size={18}/></span><b>RUST CORE</b><small>{connected?"API CONNECTED":"AWAITING LINK"}</small></div><div className="syn-branches"><div className="syn-branch"><span className="syn-wire"/><div className="syn-node"><Cpu size={17}/><b>GPU SUPERVISOR</b><small>{supervisorOnline?"AUTHENTICATED":"LOCKED / :11502"}</small></div></div><div className="syn-branch"><span className="syn-wire"/><div className="syn-node"><Server size={17}/><b>COMFYUI RUNTIME</b><small>{comfy?.status??"NOT DISCOVERED"}</small></div></div><div className="syn-branch"><span className="syn-wire"/><div className="syn-node"><Bot size={17}/><b>AGENT FABRIC</b><small>{agents.length} CONFIGURED / READ ONLY</small></div></div></div></div><div className="system-foot"><span><i className="legend-online"/>ONLINE</span><span><i className="legend-locked"/>LOCKED / NOT CONFIGURED</span><span><i className="legend-line"/>DISCOVERY LINK</span></div></section>
          <section className="panel queue-panel"><PanelTitle icon={<Workflow/>} title="MISSION QUEUE" meta="5 SEC REFRESH"/><TaskList tasks={tasks.slice(0,5)} active={active} select={setActive} action={action} busy={busy}/><button className="link" onClick={()=>setPage("tasks")}>OPEN MISSION CONTROL <ChevronRight size={14}/></button></section>
          <section className="panel service-panel"><PanelTitle icon={<Radio/>} title="LOCAL SERVICE MATRIX" meta="READ-ONLY"/>{services.length?services.map(x=><div className="row" key={x.id}><div><b>{x.name}</b><small>{x.role} · {x.latency_ms??"—"} ms</small></div><Pill value={x.status}/></div>):<Empty text="Connect to load configured local services."/>}</section>
          <section className="panel agent-panel"><PanelTitle icon={<Bot/>} title="AGENT FABRIC" meta="EXPLICIT ENDPOINTS ONLY"/>{agents.length?agents.map(x=><div className="row" key={x.id}><div><b>{x.name}</b><small>{x.adapter??x.id}</small></div><Pill value={x.status}/></div>):<Empty text="No agent health endpoints configured. No ports are scanned and no agents are launched."/>}<button className="link" onClick={()=>setPage("agents")}>INSPECT AGENT NETWORK <ChevronRight size={14}/></button></section>
        </div>}
        {page==="gpu"&&<><div className="section-head"><div><label>HARDWARE / RESOURCE DOMAIN</label><h2>GPU telemetry matrix</h2><p>Live hardware readings are shown only when the Supervisor exposes them.</p></div><Pill value={supervisorOnline?"SUPERVISOR ONLINE":"TELEMETRY LOCKED"}/></div><div className="gpu-pair large"><GpuCard index="GPU 0" title="NVIDIA RTX 3090" locked={!supervisorOnline}/><GpuCard index="GPU 1" title="NVIDIA RTX 3090" locked={!supervisorOnline}/></div><div className="panel telemetry-empty"><div className="telemetry-orb"><Gauge size={28}/></div><b>WAITING FOR HARDWARE TELEMETRY FEED</b><p>The current Rust API exposes Supervisor availability and model discovery, but not per-device utilization, VRAM, temperature, power or PCIe counters. This display will populate when a reviewed telemetry contract is connected.</p><div className="telemetry-gridline"/></div></>}
        {page==="models"&&<section className="panel"><PanelTitle icon={<Layers3/>} title="LOCAL MODEL ARSENAL" meta={`${models.length} DISCOVERED`}/>{models.length?models.map(x=><div className="row" key={x.id}><div><b>{x.id}</b><small>Discovered from local GPU gateway</small></div><Pill value="AVAILABLE"/></div>):<Empty text="No models returned by the local GPU gateway. Confirm gateway and upstream model service status."/>}</section>}
        {page==="tasks"&&<div className="task-layout"><section className="panel"><PanelTitle icon={<Workflow/>} title="ALL MISSIONS" meta={`${tasks.length} RECORDED`}/><TaskList tasks={tasks} active={active} select={setActive} action={action} busy={busy}/><button className="secondary full" disabled={!token||busy} onClick={()=>void create("service_discovery")}><Plus size={14}/>CREATE SERVICE DISCOVERY</button></section><section className="panel"><PanelTitle icon={<TerminalSquare/>} title="MISSION TRACE" meta={active?active.slice(0,8).toUpperCase():"SELECT MISSION"}/>{activeTask?<><div className="detail"><b>{activeTask.title}</b><small>{activeTask.task_type} · {activeTask.id}</small><Pill value={activeTask.status}/></div><div className="events">{events.map(x=><div className="event" key={x.sequence}><code>#{x.sequence}</code><span>{x.event_type.replaceAll("_"," ")}</span><small>{clock(x.created_at)}</small></div>)}{!events.length&&<Empty text="No mission events yet."/>}</div>{result&&<div className="result"><b>EXECUTION RESULT</b><pre>{JSON.stringify(result.result??result.error??result,null,2)}</pre></div>}</>:<Empty text="Select a mission to inspect its audit events and result."/>}</section></div>}
        {page==="agents"&&<section className="panel"><PanelTitle icon={<Bot/>} title="AGENT NETWORK" meta="READ-ONLY HEALTH PROBES"/>{agents.length?agents.map(x=><div className="row" key={x.id}><div><b>{x.name}</b><small>{x.adapter??"generic"} · {x.id}</small></div><Pill value={x.status}/></div>):<Empty text="Agent inventory is empty. Add reviewed loopback health endpoints to config/agents.toml. Discovery never scans ports or launches agents."/>}</section>}
        {page==="capabilities"&&<section className="panel"><div className="panel-toolbar"><PanelTitle icon={<ShieldCheck/>} title="PSTACK CAPABILITY INVENTORY" meta="DISCOVER / PREVIEW / APPROVE"/><button className="primary" disabled={!token||busy} onClick={()=>void discoverSkills()}><Plus size={14}/>DISCOVER & IMPORT SKILLS</button></div><p className="muted">One-click discovery imports local Markdown skills as disabled capabilities. Preview source before enabling; enabled skills are passed only as untrusted model context and never execute.</p>{notice&&<div className="approval">{notice}</div>}{skillPreview&&<div className="skill-preview"><div className="row"><div><b>REVIEW SOURCE · {skillPreview.name}</b><small>{skillPreview.id} · {skillPreview.trust} · execution: {skillPreview.execution}</small></div><button className="icon" onClick={()=>setSkillPreview(null)}><X size={14}/></button></div><p className="muted">{skillPreview.description}</p><pre className="result">{skillPreview.content}</pre><div className="compose-foot"><small>Read the full source before approving this skill.</small><button className="primary" disabled={busy} onClick={()=>void enableSkill()}><ShieldCheck size={14}/>APPROVE AS MODEL CONTEXT</button></div></div>}{capabilities.length?capabilities.map(item=><div className="row capability-row" key={item.id}><div className="capability-main"><b>{item.name}</b><small>{item.kind.replaceAll("_"," ")} · {item.id}</small><small>{item.description||"No description supplied."}</small><small>Source: {item.provenance.source_name}{item.provenance.source_version?` · ${item.provenance.source_version}`:""}{item.version?` · Capability ${item.version}`:""}</small><small>Compatibility: {item.compatibility.replaceAll("_"," ")} · Dependencies: {item.dependencies?.length?item.dependencies.join(", "):"none listed"}{item.adapter?` · Adapter: ${item.adapter}`:""}</small></div><div className="capability-state"><Pill value={item.status}/>{item.license&&<small>{item.license}</small>}{item.kind==="skill"&&<div className="capability-actions"><button className="link" disabled={busy} onClick={()=>void previewSkill(item)}>PREVIEW</button>{item.status==="enabled"?<button className="link" disabled={busy} onClick={()=>void disableSkill(item)}>DISABLE</button>:<small>Approval required</small>}</div>}</div></div>):<Empty text="No capabilities imported. Use Discover & Import Skills to scan the configured Pstack catalog."/>}</section>}
        <footer>PROJECT DREAD / UNIVERSAL OPERATIONS <span>LOOPBACK ONLY · OPERATOR APPROVAL · PYTHON GPU OWNERSHIP</span></footer>
      </div>
    </main>
  </div>;
}
function Metric({icon,label,value,detail,tone}:{icon:ReactNode;label:string;value:string;detail:string;tone:string}){return <div className={`metric ${tone}`}><div className="metric-icon">{icon}</div><small>{label}</small><b>{value}</b><span>{detail}</span><div className="metric-scan"/></div>}
function PanelTitle({icon,title,meta}:{icon:ReactNode;title:string;meta:string}){return <div className="panel-title"><div>{icon}<b>{title}</b></div><small>{meta}</small></div>}
function Empty({text}:{text:string}){return <div className="empty">{text}</div>}
function GpuCard({index,title,locked}:{index:string;title:string;locked:boolean}){return <div className="gpu-card"><div className="gpu-card-top"><div className="gpu-chip"><Cpu size={20}/></div><div><small>{index}</small><b>{title}</b></div><Pill value={locked?"LOCKED":"SUPERVISOR LINKED"}/></div><div className="gpu-scope"><div className="scope-grid"/><div className="scope-scan"/><div className="scope-center"><Activity size={19}/><b>—</b><small>NO LIVE SAMPLE</small></div></div><div className="gpu-readings"><span><small>UTILIZATION</small><b>—</b></span><span><small>VRAM</small><b>—</b></span><span><small>TEMPERATURE</small><b>—</b></span><span><small>POWER</small><b>—</b></span></div></div>}
function TaskList({tasks,active,select,action,busy}:{tasks:Task[];active:string;select:(id:string)=>void;action:(task:Task,verb:"approve"|"run"|"cancel")=>void;busy:boolean}){
  if(!tasks.length)return <Empty text="No missions have been created." />;
  return <div>{tasks.map(task=><div className={active===task.id?"task selected":"task"} key={task.id}><button className="task-name" onClick={()=>select(task.id)}><span>{task.task_type==="gpu_chat"?<MessageSquare size={15}/>:<Search size={15}/>}</span><div><b>{task.title}</b><small>{task.task_type} · {clock(task.created_at)}</small></div></button><div className="task-actions"><Pill value={task.status}/>{task.status==="pending_approval"&&<button title="Approve" disabled={busy} onClick={()=>action(task,"approve")}><Check size={14}/></button>}{task.status==="approved"&&<button title="Run" disabled={busy} onClick={()=>action(task,"run")}><Play size={14}/></button>}{["queued","running"].includes(task.status)&&<button title="Cancel" disabled={busy} onClick={()=>action(task,"cancel")}><X size={14}/></button>}</div></div>)}</div>;
}
export default App;
