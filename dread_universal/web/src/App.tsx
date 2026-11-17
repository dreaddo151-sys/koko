import { useCallback, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { Activity, AlertCircle, Bot, Check, ChevronRight, Cpu, KeyRound, LockKeyhole, LogOut, MessageSquare, Play, Plus, RefreshCw, Search, Server, ShieldCheck, TerminalSquare, X } from "lucide-react";

type Model = { id: string };
type Task = { id: string; title: string; task_type: string; status: string; created_at: number };
type Agent = { id: string; name: string; status: string; adapter?: string };
type Event = { sequence: number; event_type: string; actor: string; created_at: number };
type Result = { result?: unknown; error?: string | null; attempts?: number; elapsed_ms?: number };
type Capability = { id: string; name: string; kind: string; description: string; version?: string | null; license?: string | null; compatibility: string; dependencies: string[]; adapter?: string | null; status: string; provenance: { source_name: string; source_version?: string | null } };
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
  const tone = ["online","ok","completed","approved"].includes(s) ? "good" : ["running","queued","pending_approval"].includes(s) ? "warm" : ["failed","offline","cancelled","degraded"].includes(s) ? "bad" : "";
  return <span className={`pill ${tone}`}><i />{value.replaceAll("_"," ")}</span>;
}
function App() {
  const [token,setToken]=useState(""); const [draft,setDraft]=useState(""); const [show,setShow]=useState(false);
  const [connected,setConnected]=useState(false); const [supervisorOnline,setSupervisorConfigured]=useState(false); const [error,setError]=useState(""); const [busy,setBusy]=useState(false);
  const [models,setModels]=useState<Model[]>([]); const [model,setModel]=useState("");
  const [tasks,setTasks]=useState<Task[]>([]); const [agents,setAgents]=useState<Agent[]>([]); const [capabilities,setCapabilities]=useState<Capability[]>([]);
  const [prompt,setPrompt]=useState(""); const [active,setActive]=useState("");
  const [events,setEvents]=useState<Event[]>([]); const [result,setResult]=useState<Result|null>(null);
  const [page,setPage]=useState<"overview"|"tasks"|"agents"|"capabilities">("overview");

  const refresh=useCallback(async()=>{
    if(!token){setConnected(false);return;}
    try{
      const [health,gpuHealth,ml,tl,al,cl]=await Promise.all([
        api<{status:string}>("/health",""), api<{supervisor_online?:boolean}>("/api/gpu/health",token), api<{data?:Model[]}>("/api/gpu/models",token),
        api<{items?:Task[]}>("/api/tasks",token), api<{items?:Agent[]}>("/api/agents",token), api<{items?:Capability[]}>("/api/capabilities",token)
      ]);
      setConnected(health.status==="ok"); setSupervisorConfigured(Boolean(gpuHealth.supervisor_online)); setModels(ml.data??[]); setTasks(tl.items??[]); setAgents(al.items??[]); setCapabilities(cl.items??[]); setError("");
    }catch(e){setConnected(false);setError(e instanceof Error?e.message:"API request failed");}
  },[token]);
  useEffect(()=>{void refresh();if(!token)return;const t=window.setInterval(()=>void refresh(),5000);return()=>window.clearInterval(t);},[refresh,token]);
  useEffect(()=>{if(models.length&&!models.some(x=>x.id===model))setModel(models[0].id);},[models,model]);
  useEffect(()=>{
    if(!token||!active){setEvents([]);setResult(null);return;}
    let dead=false;
    const load=async()=>{try{
      const [ev,res]=await Promise.all([
        api<{items?:Event[]}>(`/api/tasks/${active}/events`,token),
        api<Result>(`/api/tasks/${active}/result`,token).catch(()=>null)
      ]);
      if(!dead){setEvents(ev.items??[]);setResult(res);}
    }catch{if(!dead)setEvents([]);}};
    void load();const t=window.setInterval(()=>void load(),2500);
    return()=>{dead=true;window.clearInterval(t);};
  },[active,token]);
  const stats=useMemo(()=>({
    active:tasks.filter(x=>["queued","running"].includes(x.status)).length,
    pending:tasks.filter(x=>x.status==="pending_approval").length,
    done:tasks.filter(x=>x.status==="completed").length
  }),[tasks]);
  const connect=()=>{if(!draft.trim()){setError("Enter DREAD_API_TOKEN.");return;}setToken(draft.trim());setDraft("");};
  const create=async(type:"gpu_chat"|"service_discovery")=>{
    if(!token)return;if(type==="gpu_chat"&&!supervisorOnline){setError("GPU inference is locked until the Python GPU Supervisor bridge is online and authenticated.");return;}if(type==="gpu_chat"&&(!model||!prompt.trim())){setError("Select a model and enter a prompt.");return;}
    setBusy(true);setError("");
    try{
      const payload=type==="gpu_chat"?{model,messages:[{role:"user",content:prompt.trim()}],max_tokens:1024}:{};
      const out=await api<{item:Task}>("/api/tasks",token,{method:"POST",body:JSON.stringify({title:type==="gpu_chat"?`Chat · ${model}`:"Local service discovery",task_type:type,payload})});
      setTasks(x=>[out.item,...x]);setActive(out.item.id);setPage("tasks");if(type==="gpu_chat")setPrompt("");
    }catch(e){setError(e instanceof Error?e.message:"Task creation failed");}finally{setBusy(false);}
  };
  const action=async(task:Task,verb:"approve"|"run"|"cancel")=>{
    setBusy(true);setError("");
    try{await api(`/api/tasks/${task.id}/${verb}`,token,{method:"POST",body:verb==="approve"?JSON.stringify({reason:"Approved in DREAD Universal Console"}):undefined});await refresh();setActive(task.id);}
    catch(e){setError(e instanceof Error?e.message:`Task ${verb} failed`);}finally{setBusy(false);}
  };
  const activeTask=tasks.find(x=>x.id===active);
  return <div className="shell">
    <aside className="sidebar">
      <div className="brand"><div className="brand-icon"><TerminalSquare size={22}/></div><div><b>DREAD</b><small>UNIVERSAL / 0.4</small></div></div>
      <div className="side-label">COMMAND</div>
      <button className={page==="overview"?"nav active":"nav"} onClick={()=>setPage("overview")}><Activity size={17}/>Overview</button>
      <button className={page==="tasks"?"nav active":"nav"} onClick={()=>setPage("tasks")}><TerminalSquare size={17}/>Task queue <span>{tasks.length}</span></button>
      <button className={page==="agents"?"nav active":"nav"} onClick={()=>setPage("agents")}><Bot size={17}/>Agents</button>
      <button className={page==="capabilities"?"nav active":"nav"} onClick={()=>setPage("capabilities")}><ShieldCheck size={17}/>Capabilities <span>{capabilities.length}</span></button>
      <div className="safety"><ShieldCheck size={18}/><div><b>Safety boundary</b><small>Loopback API · approval required · Python Supervisor owns GPU leases</small></div></div>
      <div className="side-foot">LOCAL-FIRST CONTROL PLANE<br/>NO REMOTE EXECUTION</div>
    </aside>
    <main>
      <header className="topbar"><div>DREAD SYSTEM <ChevronRight size={13}/> <b>{page.toUpperCase()}</b></div><div className="topright"><span className={connected?"online":"offline"}><i/>{connected?"API CONNECTED":token?"API OFFLINE":"LOCKED"}</span><button className="icon" onClick={()=>void refresh()} title="Refresh"><RefreshCw size={15}/></button>{token&&<button className="icon" title="Disconnect and clear token" onClick={()=>{setToken("");setPage("overview");setActive("");setEvents([]);setResult(null);}}><LogOut size={15}/></button>}</div></header>
      <div className="content">
        <div className="heading"><div><label>LOCAL AI OPERATIONS / COMMAND HUB</label><h1>{page==="overview"?"Command overview":page==="tasks"?"Task operations":page==="agents"?"Agent directory":"Capability registry"}</h1><p>Observe, approve and operate your local AI stack without bypassing its safety boundaries.</p></div><div className="workstation"><i/>DUAL GPU WORKSTATION PROFILE</div></div>
        {!token&&<div className="connect"><LockKeyhole size={21}/><div><b>Unlock local control plane</b><small>API token stays in page memory; it is never written to browser storage.</small></div><div className="token"><KeyRound size={15}/><input type={show?"text":"password"} value={draft} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>e.key==="Enter"&&connect()} placeholder="DREAD_API_TOKEN"/><button className="icon" onClick={()=>setShow(!show)}><Search size={14}/></button><button className="primary" onClick={connect}>Connect</button></div></div>}
        {error&&<div className="error"><AlertCircle size={16}/>{error}<button onClick={()=>setError("")}><X size={14}/></button></div>}
        <div className="metrics">
          <Metric icon={<Cpu/>} label="GPU supervisor" value={supervisorOnline?"Ready":"Locked"} detail="Python lease bridge · 127.0.0.1:11502"/>
          <Metric icon={<Activity/>} label="Active tasks" value={String(stats.active)} detail={`${stats.pending} awaiting approval`}/>
          <Metric icon={<MessageSquare/>} label="Available models" value={String(models.length)} detail="Discovered from local gateway"/>
          <Metric icon={<Server/>} label="Configured agents" value={String(agents.length)} detail={`${stats.done} completed tasks`}/>
        </div>
        {page==="overview"&&<div className="grid">
          <section className="panel compose"><PanelTitle icon={<MessageSquare/>} title="New local inference" meta="APPROVAL-GATED"/>
            <label>MODEL ROUTE</label><select value={model} onChange={e=>setModel(e.target.value)} disabled={!token||!models.length}><option value="">Connect to discover models</option>{models.map(x=><option key={x.id}>{x.id}</option>)}</select>
            <label>PROMPT</label><textarea rows={5} value={prompt} onChange={e=>setPrompt(e.target.value)} placeholder="Ask your local model to reason, write, analyze or help..." disabled={!token}/>
            <div className="compose-foot"><small>Text-only · non-streaming · result capped at 1 MiB</small><button className="primary" disabled={!token||!supervisorOnline||busy||!model||!prompt.trim()} onClick={()=>void create("gpu_chat")}><Plus size={14}/>Create task</button></div>
            <div className="approval"><ShieldCheck size={14}/>{supervisorOnline?"Creating a task does not execute inference. Approve it first.":"GPU inference is locked until the Python Supervisor bridge is online and authenticated."}</div>
          </section>
          <section className="panel"><PanelTitle icon={<Activity/>} title="Task queue" meta="5 SEC POLL"/><TaskList tasks={tasks.slice(0,6)} active={active} select={setActive} action={action} busy={busy}/><button className="link" onClick={()=>setPage("tasks")}>Open all tasks <ChevronRight size={14}/></button></section>
          <section className="panel"><PanelTitle icon={<Bot/>} title="Agent health" meta="READ-ONLY PROBES"/>{agents.length?agents.map(x=><div className="row" key={x.id}><div><b>{x.name}</b><small>{x.adapter??x.id}</small></div><Pill value={x.status}/></div>):<Empty text="No agent health endpoints configured."/>}</section>
          <section className="panel"><PanelTitle icon={<Search/>} title="Service discovery" meta="OPERATOR-INITIATED"/><p className="muted">Create a read-only scan of explicitly configured local service endpoints.</p><button className="secondary" disabled={!token||busy} onClick={()=>void create("service_discovery")}><Plus size={14}/>Create discovery task</button></section>
        </div>}
        {page==="tasks"&&<div className="task-layout"><section className="panel"><PanelTitle icon={<Activity/>} title="All tasks" meta={`${tasks.length} RECORDED`}/><TaskList tasks={tasks} active={active} select={setActive} action={action} busy={busy}/><button className="secondary full" disabled={!token||busy} onClick={()=>void create("service_discovery")}><Plus size={14}/>Create service discovery task</button></section>
          <section className="panel"><PanelTitle icon={<TerminalSquare/>} title="Task detail" meta={active?active.slice(0,8).toUpperCase():"SELECT A TASK"}/>{activeTask?<><div className="detail"><b>{activeTask.title}</b><small>{activeTask.task_type} · {activeTask.id}</small><Pill value={activeTask.status}/></div><div className="events">{events.map(x=><div className="event" key={x.sequence}><code>#{x.sequence}</code><span>{x.event_type.replaceAll("_"," ")}</span><small>{clock(x.created_at)}</small></div>)}{!events.length&&<Empty text="No task events yet."/>}</div>{result&&<div className="result"><b>Execution result</b><pre>{JSON.stringify(result.result??result.error??result,null,2)}</pre></div>}</>:<Empty text="Select a task to inspect its audit events and result."/>}</section></div>}
        {page==="agents"&&<section className="panel"><PanelTitle icon={<Bot/>} title="Configured agents" meta="EXPLICIT HEALTH URLS ONLY"/>{agents.length?agents.map(x=><div className="row" key={x.id}><div><b>{x.name}</b><small>{x.adapter??"generic"} · {x.id}</small></div><Pill value={x.status}/></div>):<Empty text="Agent inventory is empty. Add reviewed loopback health endpoints to config/agents.toml; discovery never scans ports or launches agents."/>}</section>
        {page==="capabilities"&&<section className="panel"><PanelTitle icon={<ShieldCheck/>} title="Imported capability inventory" meta="METADATA ONLY · DISABLED BY DEFAULT"/><p className="muted">Review provenance, compatibility and dependencies. Importing a capability never executes its code; activation is deliberately not available in this console.</p>{capabilities.length?capabilities.map(item=><div className="row capability-row" key={item.id}><div className="capability-main"><b>{item.name}</b><small>{item.kind.replaceAll("_"," ")} · {item.id}</small><small>{item.description||"No description supplied."}</small><small>Source: {item.provenance.source_name}{item.provenance.source_version?` · ${item.provenance.source_version}`:""}{item.version?` · Capability ${item.version}`:""}</small><small>Compatibility: {item.compatibility.replaceAll("_"," ")} · Dependencies: {item.dependencies?.length?item.dependencies.join(", "):"none listed"}{item.adapter?` · Adapter: ${item.adapter}`:""}</small></div><div className="capability-state"><Pill value={item.status}/>{item.license&&<small>{item.license}</small>}</div></div>):<Empty text="No capabilities imported. The pinned Pstack inventory can be imported separately; imported records remain disabled until reviewed."/ >}</section>}}
        <footer>PROJECT DREAD // UNIVERSAL CONTROL PLANE <span>LOOPBACK ONLY · APPROVAL REQUIRED · NO PLUGIN EXECUTION</span></footer>
      </div>
    </main>
  </div>;
}
function Metric({icon,label,value,detail}:{icon:ReactNode;label:string;value:string;detail:string}){return <div className="metric"><div>{icon}</div><small>{label}</small><b>{value}</b><span>{detail}</span></div>}
function PanelTitle({icon,title,meta}:{icon:ReactNode;title:string;meta:string}){return <div className="panel-title"><div>{icon}<b>{title}</b></div><small>{meta}</small></div>}
function Empty({text}:{text:string}){return <div className="empty">{text}</div>}
function TaskList({tasks,active,select,action,busy}:{tasks:Task[];active:string;select:(id:string)=>void;action:(task:Task,verb:"approve"|"run"|"cancel")=>void;busy:boolean}){
  if(!tasks.length)return <Empty text="No tasks have been created yet."/>;
  return <div>{tasks.map(task=><div className={active===task.id?"task selected":"task"} key={task.id}>
    <button className="task-name" onClick={()=>select(task.id)}><span>{task.task_type==="gpu_chat"?<MessageSquare size={15}/>:<Search size={15}/>}</span><div><b>{task.title}</b><small>{task.task_type} · {clock(task.created_at)}</small></div></button>
    <div className="task-actions"><Pill value={task.status}/>{task.status==="pending_approval"&&<button title="Approve" disabled={busy} onClick={()=>action(task,"approve")}><Check size={14}/></button>}{task.status==="approved"&&<button title="Run" disabled={busy} onClick={()=>action(task,"run")}><Play size={14}/></button>}{["queued","running"].includes(task.status)&&<button title="Cancel" disabled={busy} onClick={()=>action(task,"cancel")}><X size={14}/></button>}</div>
  </div>)}</div>;
}
export default App;
