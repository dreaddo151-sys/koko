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
type GpuDevice = { vendor: string; name: string; vram_mib: number | null; vram_used_mib: number | null; vram_free_mib: number | null; driver_version: string | null; pcie_link_gen: number | null; pcie_link_width: number | null; pcie_link_gen_max: number | null; pcie_link_width_max: number | null; utilization_percent: number | null; temperature_c: number | null; power_draw_mw: number | null };
type HostProfile = { os: string; architecture: string; distribution: string | null; distribution_version: string | null; kernel_release: string | null; cpu_threads: number; memory_total_mib: number | null; memory_available_mib: number | null; root_disk_total_mib: number | null; root_disk_available_mib: number | null; gpu_devices: GpuDevice[]; llama_server: string | null; nvidia_smi_available: boolean; rocm_smi_available: boolean; xpu_smi_available: boolean; inference_backend: string };
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
const mib = (value: number | null) => value === null ? "NOT REPORTED" : `${(value / 1024).toFixed(1)} GiB`;

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
  const [skillPreview,setSkillPreview]=useState<SkillPreview|null>(null); const [notice,setNotice]=useState(""); const [selectedSkills,setSelectedSkills]=useState<string[]>([]); const [hostProfile,setHostProfile]=useState<HostProfile|null>(null);
  const [prompt,setPrompt]=useState(""); const [active,setActive]=useState("");
  const [events,setEvents]=useState<Event[]>([]); const [result,setResult]=useState<Result|null>(null);
  const [page,setPage]=useState<Page>("overview"); const [lastRefresh,setLastRefresh]=useState<number|null>(null);

  const refresh=useCallback(async()=>{
    if(!token){setConnected(false);return;}
    try{
      const [health,gpuHealth,ml,tl,al,cl,sv,hp]=await Promise.all([
        api<{status:string}>("/health",""), api<{supervisor_online?:boolean}>("/api/gpu/health",token), api<{data?:Model[]}>("/api/gpu/models",token),
        api<{items?:Task[]}>("/api/tasks",token), api<{items?:Agent[]} >("/api/agents",token), api<{items?:Capability[]}>("/api/capabilities",token), api<Service[]>("/api/services",token), api<HostProfile>("/api/system/profile",token)
      ]);
      setConnected(health.status==="ok"); setSupervisorConfigured(Boolean(gpuHealth.supervisor_online)); setModels(ml.data??[]); setTasks(tl.items??[]); setAgents(al.items??[]); setCapabilities(cl.items??[]); setServices(sv??[]); setHostProfile(hp); setError(""); setLastRefresh(Date.now());
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
      setNotice(`Pstack scan: ${found.count} skills and agent profiles found · ${imported.imported} imported · ${imported.already_present} already present. All imports remain disabled.`);
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
        <div className="eyebrow"><span className="pulse-dot"/> LOCAL AI OPERATIONS / {hostProfile?.gpu_devices.length??0} GPU DEVICE(S) <span className="eyebrow-line"/></div>
        <div className="heading"><div><h1>{page==="overview"?"Command deck":page==="tasks"?"Mission control":page==="agents"?"Agent network":page==="capabilities"?"Pstack capability registry":page==="models"?"Model arsenal":"GPU telemetry"}</h1><p>Observe, coordinate and operate your local AI stack without crossing its safety boundaries.</p></div><div className="workstation"><span className="workstation-mark"><Cpu size={15}/></span><div><b>{hostProfile?`${hostProfile.gpu_devices.length} GPU DEVICE(S) / ${(hostProfile.distribution??hostProfile.os).toUpperCase()}`:"HOST DISCOVERY PENDING"}</b><small>{hostProfile?`${hostProfile.architecture.toUpperCase()} · ${hostProfile.cpu_threads} CPU THREADS`:"LOCAL NODE / LOOPBACK"}</small></div><Pill value={connected?"LINKED":"LOCKED"}/></div></div>
        {!token&&<div className="connect"><LockKeyhole size={21}/><div><b>AUTHENTICATE COMMAND LINK</b><small>Bearer token remains in page memory only. Never persisted to browser storage.</small></div><div className="token"><KeyRound size={15}/><input type={show?"text":"password"} value={draft} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>e.key==="Enter"&&connect()} placeholder="DREAD API ACCESS TOKEN"/><button className="icon" onClick={()=>setShow(!show)}><Search size={14}/></button><button className="primary" onClick={connect}>ESTABLISH LINK</button></div></div>}
        {error&&<div className="error"><AlertCircle size={16}/>{error}<button onClick={()=>setError("")}><X size={14}/></button></div>}
        {hostProfile&&<section className="panel host-panel"><PanelTitle icon={<Cpu/>} title="AUTO-DISCOVERED HOST PROFILE" meta="READ-ONLY / NO HOST MUTATIONS"/><div className="host-grid"><div><small>UBUNTU / DISTRIBUTION</small><b>{hostProfile.distribution??hostProfile.os} {hostProfile.distribution_version??""} · {hostProfile.architecture}</b></div><div><small>KERNEL</small><b>{hostProfile.kernel_release??"NOT REPORTED"}</b></div><div><small>CPU THREADS</small><b>{hostProfile.cpu_threads}</b></div><div><small>RAM TOTAL / AVAILABLE</small><b>{mib(hostProfile.memory_total_mib)} / {mib(hostProfile.memory_available_mib)}</b></div><div><small>ROOT DISK TOTAL / FREE</small><b>{mib(hostProfile.root_disk_total_mib)} / {mib(hostProfile.root_disk_available_mib)}</b></div><div><small>INFERENCE BACKEND</small><b>{hostProfile.inference_backend}</b></div><div><small>LLAMA SERVER</small><b>{hostProfile.llama_server??"NOT FOUND IN PATH"}</b></div></div><div className="host-gpus">{hostProfile.gpu_devices.length?hostProfile.gpu_devices.map((gpu,index)=><div className="row" key={`${gpu.vendor}-${index}`}><div><b>{gpu.name}</b><small>{gpu.vendor.toUpperCase()} · {gpu.vram_used_mib===null?"VRAM USE N/A":`${gpu.vram_used_mib.toLocaleString()}/${gpu.vram_mib?.toLocaleString()??"?"} MiB · ${gpu.vram_free_mib?.toLocaleString()??"?"} MiB FREE`} · {gpu.driver_version?`DRIVER ${gpu.driver_version}`:"DRIVER N/A"} · {gpu.pcie_link_gen?`PCIe CURRENT Gen${gpu.pcie_link_gen} x${gpu.pcie_link_width??"?"} / MAX Gen${gpu.pcie_link_gen_max??"?"} x${gpu.pcie_link_width_max??"?"}`:"PCIe N/A"} · {gpu.utilization_percent===null?"LOAD N/A":`${gpu.utilization_percent}% LOAD`} · {gpu.temperature_c===null?"TEMP N/A":`${gpu.temperature_c}°C`} · {gpu.power_draw_mw===null?"POWER N/A":`${(gpu.power_draw_mw/1000).toFixed(1)} W`}</small></div><Pill value="DETECTED"/></div>):<Empty text="No supported GPU utility reported a device. CPU-only host remains discoverable."/>}</div><div className="host-tools"><span>NVIDIA-SMI {hostProfile.nvidia_smi_available?"FOUND":"MISSING"}</span><span>ROCM-SMI {hostProfile.rocm_smi_available?"FOUND":"MISSING"}</span><span>XPU-SMI {hostProfile.xpu_smi_available?"FOUND":"MISSING"}</span><span>PCIe link speed may downshift while idle; compare CURRENT with MAX.</span></div></section>}
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
            <div className="skill-picker"><label>APPROVED SKILL CONTEXT / OPTIONAL</label>{capabilities.filter(x=>(x.kind==="skill"||x.kind==="agent")&&x.status==="enabled").length?capabilities.filter(x=>(x.kind==="skill"||x.kind==="agent")&&x.status==="enabled").map(skill=><label className="skill-choice" key={skill.id}><input type="checkbox" checked={selectedSkills.includes(skill.id)} onChange={e=>setSelectedSkills(current=>e.target.checked?[...current,skill.id]:current.filter(id=>id!==skill.id))}/><span>{skill.name}<small>{skill.id}</small></span></label>):<small>No enabled skills or agent profiles. Import, preview and approve them in the Pstack registry.</small>}</div>
            <div className="compose-foot"><small>TEXT ONLY / NON-STREAMING / RESULT CAP 1 MiB</small><button className="primary" disabled={!token||!supervisorOnline||busy||!model||!prompt.trim()} onClick={()=>void create("gpu_chat")}><Plus size={14}/>QUEUE INFERENCE</button></div>
            <div className="approval"><ShieldCheck size={14}/>{supervisorOnline?"Task creation does not execute inference. Operator approval is required.":"GPU inference locked — authenticated Python Supervisor bridge unavailable."}</div>
          </section>
          <section className="panel gpu-strip"><PanelTitle icon={<Gauge/>} title="DETECTED GPU ARRAY" meta="HOST PROFILE / TELEMETRY OPTIONAL"/><div className="gpu-pair">{hostProfile?.gpu_devices.length?hostProfile.gpu_devices.map((gpu,index)=><GpuCard key={`${gpu.vendor}-${index}`} index={`GPU ${index}`} gpu={gpu}/>):<Empty text="No supported GPU detected. The control plane remains available for CPU-only llama.cpp when configured."/>}</div><div className="telemetry-note"><Activity size={13}/> Read-only hardware samples come from host discovery. The Python GPU Supervisor separately owns inference admission and process lifecycle.</div><button className="link" onClick={()=>setPage("gpu")}>OPEN GPU TELEMETRY <ChevronRight size={14}/></button></section>
          <section className="panel system-panel"><PanelTitle icon={<Workflow/>} title="SYSTEM SYNAPSE" meta="LIVE SERVICE MAP"/><div className="synapse"><div className="syn-node hub-node"><span className="node-core"><Activity size={18}/></span><b>RUST CORE</b><small>{connected?"API CONNECTED":"AWAITING LINK"}</small></div><div className="syn-branches"><div className="syn-branch"><span className="syn-wire"/><div className="syn-node"><Cpu size={17}/><b>GPU SUPERVISOR</b><small>{supervisorOnline?"AUTHENTICATED":"LOCKED / :11502"}</small></div></div><div className="syn-branch"><span className="syn-wire"/><div className="syn-node"><Server size={17}/><b>COMFYUI RUNTIME</b><small>{comfy?.status??"NOT DISCOVERED"}</small></div></div><div className="syn-branch"><span className="syn-wire"/><div className="syn-node"><Bot size={17}/><b>AGENT FABRIC</b><small>{agents.length} CONFIGURED / READ ONLY</small></div></div></div></div><div className="system-foot"><span><i className="legend-online"/>ONLINE</span><span><i className="legend-locked"/>LOCKED / NOT CONFIGURED</span><span><i className="legend-line"/>DISCOVERY LINK</span></div></section>
          <section className="panel queue-panel"><PanelTitle icon={<Workflow/>} title="MISSION QUEUE" meta="5 SEC REFRESH"/><TaskList tasks={tasks.slice(0,5)} active={active} select={setActive} action={action} busy={busy}/><button className="link" onClick={()=>setPage("tasks")}>OPEN MISSION CONTROL <ChevronRight size={14}/></button></section>
          <section className="panel service-panel"><PanelTitle icon={<Radio/>} title="LOCAL SERVICE MATRIX" meta="READ-ONLY"/>{services.length?services.map(x=><div className="row" key={x.id}><div><b>{x.name}</b><small>{x.role} · {x.latency_ms??"—"} ms</small></div><Pill value={x.status}/></div>):<Empty text="Connect to load configured local services."/>}</section>
          <section className="panel agent-panel"><PanelTitle icon={<Bot/>} title="AGENT FABRIC" meta="ALLOWLISTED LOCAL / EXPLICIT"/>{agents.length?agents.map(x=><div className="row" key={x.id}><div><b>{x.name}</b><small>{x.adapter??x.id}</small></div><Pill value={x.status}/></div>):<Empty text="No known local agent endpoint responded. DREAD probes only allowlisted loopback health URLs; it does not sweep ports or launch agents."/>}<button className="link" onClick={()=>setPage("agents")}>INSPECT AGENT NETWORK <ChevronRight size={14}/></button></section>
        </div>}
        {page==="gpu"&&<><div className="section-head"><div><label>HARDWARE / RESOURCE DOMAIN</label><h2>GPU telemetry matrix</h2><p>Read-only host samples are displayed independently of inference lease availability.</p></div><Pill value={supervisorOnline?"SUPERVISOR ONLINE":"SUPERVISOR OFFLINE"}/></div><div className="gpu-pair large">{hostProfile?.gpu_devices.length?hostProfile.gpu_devices.map((gpu,index)=><GpuCard key={`${gpu.vendor}-${index}`} index={`GPU ${index}`} gpu={gpu}/>):<Empty text="No supported GPU detected. CPU inference remains available when a local llama.cpp runtime is configured."/>}</div><div className="telemetry-note"><Activity size={13}/> Hardware telemetry is sampled by the host discovery layer. Supervisor status controls inference leases, not whether read-only GPU metrics may be displayed.</div></>}
        {page==="models"&&<section className="panel"><PanelTitle icon={<Layers3/>} title="LOCAL MODEL ARSENAL" meta={`${models.length} DISCOVERED`}/>{models.length?models.map(x=><div className="row" key={x.id}><div><b>{x.id}</b><small>Discovered from local GPU gateway</small></div><Pill value="AVAILABLE"/></div>):<Empty text="No models returned by the local GPU gateway. Confirm gateway and upstream model service status."/>}</section>}
        {page==="tasks"&&<div className="task-layout"><section className="panel"><PanelTitle icon={<Workflow/>} title="ALL MISSIONS" meta={`${tasks.length} RECORDED`}/><TaskList tasks={tasks} active={active} select={setActive} action={action} busy={busy}/><button className="secondary full" disabled={!token||busy} onClick={()=>void create("service_discovery")}><Plus size={14}/>CREATE SERVICE DISCOVERY</button></section><section className="panel"><PanelTitle icon={<TerminalSquare/>} title="MISSION TRACE" meta={active?active.slice(0,8).toUpperCase():"SELECT MISSION"}/>{activeTask?<><div className="detail"><b>{activeTask.title}</b><small>{activeTask.task_type} · {activeTask.id}</small><Pill value={activeTask.status}/></div><div className="events">{events.map(x=><div className="event" key={x.sequence}><code>#{x.sequence}</code><span>{x.event_type.replaceAll("_"," ")}</span><small>{clock(x.created_at)}</small></div>)}{!events.length&&<Empty text="No mission events yet."/>}</div>{result&&<div className="result"><b>EXECUTION RESULT</b><pre>{JSON.stringify(result.result??result.error??result,null,2)}</pre></div>}</>:<Empty text="Select a mission to inspect its audit events and result."/>}</section></div>}
        {page==="agents"&&<section className="panel"><PanelTitle icon={<Bot/>} title="AGENT NETWORK" meta="READ-ONLY HEALTH PROBES"/>{agents.length?agents.map(x=><div className="row" key={x.id}><div><b>{x.name}</b><small>{x.adapter??"generic"} · {x.id}</small></div><Pill value={x.status}/></div>):<Empty text="Agent inventory is empty. Add reviewed loopback health endpoints to config/agents.toml. Discovery never scans ports or launches agents."/>}</section>}
        {page==="capabilities"&&<section className="panel"><div className="panel-toolbar"><PanelTitle icon={<ShieldCheck/>} title="PSTACK CAPABILITY INVENTORY" meta="DISCOVER / PREVIEW / APPROVE"/><button className="primary" disabled={!token||busy} onClick={()=>void discoverSkills()}><Plus size={14}/>DISCOVER & IMPORT SKILLS</button></div><p className="muted">One-click discovery imports local Markdown skills as disabled capabilities. Preview source before enabling; enabled skills are passed only as untrusted model context and never execute.</p>{notice&&<div className="approval">{notice}</div>}{skillPreview&&<div className="skill-preview"><div className="row"><div><b>REVIEW SOURCE · {skillPreview.name}</b><small>{skillPreview.id} · {skillPreview.trust} · execution: {skillPreview.execution}</small></div><button className="icon" onClick={()=>setSkillPreview(null)}><X size={14}/></button></div><p className="muted">{skillPreview.description}</p><pre className="result">{skillPreview.content}</pre><div className="compose-foot"><small>Read the full source before approving this skill.</small><button className="primary" disabled={busy} onClick={()=>void enableSkill()}><ShieldCheck size={14}/>APPROVE AS MODEL CONTEXT</button></div></div>}{capabilities.length?capabilities.map(item=><div className="row capability-row" key={item.id}><div className="capability-main"><b>{item.name}</b><small>{item.kind.replaceAll("_"," ")} · {item.id}</small><small>{item.description||"No description supplied."}</small><small>Source: {item.provenance.source_name}{item.provenance.source_version?` · ${item.provenance.source_version}`:""}{item.version?` · Capability ${item.version}`:""}</small><small>Compatibility: {item.compatibility.replaceAll("_"," ")} · Dependencies: {item.dependencies?.length?item.dependencies.join(", "):"none listed"}{item.adapter?` · Adapter: ${item.adapter}`:""}</small></div><div className="capability-state"><Pill value={item.status}/>{item.license&&<small>{item.license}</small>}{(item.kind==="skill"||item.kind==="agent")&&<div className="capability-actions"><button className="link" disabled={busy} onClick={()=>void previewSkill(item)}>PREVIEW</button>{item.status==="enabled"?<button className="link" disabled={busy} onClick={()=>void disableSkill(item)}>DISABLE</button>:<small>Approval required</small>}</div>}</div></div>):<Empty text="No capabilities imported. Use Discover & Import Skills to scan the configured Pstack catalog."/>}</section>}
        <footer>PROJECT DREAD / UNIVERSAL OPERATIONS <span>LOOPBACK ONLY · OPERATOR APPROVAL · PYTHON GPU OWNERSHIP</span></footer>
      </div>
    </main>
  </div>;
}
function Metric({icon,label,value,detail,tone}:{icon:ReactNode;label:string;value:string;detail:string;tone:string}){return <div className={`metric ${tone}`}><div className="metric-icon">{icon}</div><small>{label}</small><b>{value}</b><span>{detail}</span><div className="metric-scan"/></div>}
function PanelTitle({icon,title,meta}:{icon:ReactNode;title:string;meta:string}){return <div className="panel-title"><div>{icon}<b>{title}</b></div><small>{meta}</small></div>}
function Empty({text}:{text:string}){return <div className="empty">{text}</div>}
function GpuCard({index,gpu}:{index:string;gpu:GpuDevice}){
  const hasTelemetry=gpu.utilization_percent!==null||gpu.vram_used_mib!==null||gpu.temperature_c!==null||gpu.power_draw_mw!==null;
  const vram=gpu.vram_used_mib===null||gpu.vram_mib===null?"N/A":`${gpu.vram_used_mib.toLocaleString()} / ${gpu.vram_mib.toLocaleString()} MiB`;
  const utilization=gpu.utilization_percent===null?"N/A":`${gpu.utilization_percent}%`;
  const temperature=gpu.temperature_c===null?"N/A":`${gpu.temperature_c}°C`;
  const power=gpu.power_draw_mw===null?"N/A":`${(gpu.power_draw_mw/1000).toFixed(1)} W`;
  const pcie=gpu.pcie_link_gen===null?"PCIe N/A":`PCIe Gen${gpu.pcie_link_gen} x${gpu.pcie_link_width??"?"} / Max Gen${gpu.pcie_link_gen_max??"?"} x${gpu.pcie_link_width_max??"?"}`;
  return <div className="gpu-card"><div className="gpu-card-top"><div className="gpu-chip"><Cpu size={20}/></div><div><small>{index} · {gpu.vendor.toUpperCase()}</small><b>{gpu.name}</b><small>{gpu.driver_version? `DRIVER ${gpu.driver_version}`:"DRIVER N/A"} · {pcie}</small></div><Pill value={hasTelemetry?"TELEMETRY AVAILABLE":"DETECTED / N/A"}/></div><div className="gpu-scope"><div className="scope-grid"/><div className="scope-scan"/><div className="scope-center"><Activity size={19}/><b>{utilization}</b><small>GPU UTILIZATION</small></div></div><div className="gpu-readings"><span><small>UTILIZATION</small><b>{utilization}</b></span><span><small>VRAM USED / TOTAL</small><b>{vram}</b></span><span><small>TEMPERATURE</small><b>{temperature}</b></span><span><small>POWER DRAW</small><b>{power}</b></span></div></div>;
}
function TaskList({tasks,active,select,action,busy}:{tasks:Task[];active:string;select:(id:string)=>void;action:(task:Task,verb:"approve"|"run"|"cancel")=>void;busy:boolean}){
  if(!tasks.length)return <Empty text="No missions have been created." />;
  return <div>{tasks.map(task=><div className={active===task.id?"task selected":"task"} key={task.id}><button className="task-name" onClick={()=>select(task.id)}><span>{task.task_type==="gpu_chat"?<MessageSquare size={15}/>:<Search size={15}/>}</span><div><b>{task.title}</b><small>{task.task_type} · {clock(task.created_at)}</small></div></button><div className="task-actions"><Pill value={task.status}/>{task.status==="pending_approval"&&<button title="Approve" disabled={busy} onClick={()=>action(task,"approve")}><Check size={14}/></button>}{task.status==="approved"&&<button title="Run" disabled={busy} onClick={()=>action(task,"run")}><Play size={14}/></button>}{["queued","running"].includes(task.status)&&<button title="Cancel" disabled={busy} onClick={()=>action(task,"cancel")}><X size={14}/></button>}</div></div>)}</div>;
}
export default App;
