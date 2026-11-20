import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Activity, AlertCircle, Bot, Check, ChevronRight, Cpu, FolderSearch, Gauge, KeyRound, Layers3, LockKeyhole, LogOut, MessageSquare, Play, Plus, Radio, RefreshCw, Search, Server, ShieldCheck, TerminalSquare, X, Zap, Workflow } from "lucide-react";

type Model = { id: string };
type Task = { id: string; title: string; task_type: string; status: string; created_at: number };
type MissionNode = { id: string; depends_on?: string[]; priority?: number; task_type: string; payload?: unknown };
type MissionSummary = { mission_id: string; status: string; created_at: number; node_count: number };
type MissionTask = { node_id: string; task_id: string; depends_on: string[]; status: string; result?: unknown; error?: string | null; attempts: number; elapsed_ms: number };
type MissionDetail = { mission_id: string; graph: { mission_id: string; nodes: MissionNode[] }; execution_order: string[]; tasks: MissionTask[]; status: string; created_at: number; execution_started: boolean };
type Agent = { id: string; name: string; status: string; adapter?: string };
type Event = { sequence: number; event_type: string; actor: string; created_at: number; payload?: Record<string, unknown> };
type Result = { result?: unknown; error?: string | null; attempts?: number; elapsed_ms?: number };
type Capability = { id: string; name: string; kind: string; description: string; version?: string | null; license?: string | null; compatibility: string; dependencies: string[]; adapter?: string | null; status: string; provenance: { source_name: string; source_version?: string | null } };
type SkillPreview = { id: string; name: string; description: string; content: string; trust: string; execution: string };
type Service = { id: string; name: string; role: string; status: string; http_status?: number | null; latency_ms?: number; summary?: string | null; detail?: string };
type GpuDevice = { vendor: string; name: string; device_id: string | null; device_index: number | null; pci_bus_id: string | null; compute_capability: string | null; vram_mib: number | null; vram_used_mib: number | null; vram_free_mib: number | null; driver_version: string | null; pcie_link_gen: number | null; pcie_link_width: number | null; pcie_link_gen_max: number | null; pcie_link_width_max: number | null; utilization_percent: number | null; temperature_c: number | null; power_draw_mw: number | null };
type StorageDevice = { source: string; filesystem: string; mount_point: string; total_mib: number | null; used_mib: number | null; available_mib: number | null };
type StoragePartition = { name: string; filesystem: string | null; mount_points: string[] };
type PhysicalStorageDevice = { name: string; model: string | null; transport: string | null; size_mib: number | null; partitions: StoragePartition[] };
type HostProfile = { os: string; architecture: string; distribution: string | null; distribution_version: string | null; kernel_release: string | null; cpu_model: string | null; cpu_physical_cores: number | null; cpu_logical_threads: number | null; cpu_sockets: number | null; cpu_cores_per_socket: number | null; cpu_threads_per_core: number | null; cpu_numa_nodes: number | null; cuda_version: string | null; cuda_toolkit_version: string | null; cuda_toolkits: {version: string; compiler_path: string}[]; cpu_threads: number; memory_total_mib: number | null; memory_available_mib: number | null; root_disk_total_mib: number | null; root_disk_available_mib: number | null; storage_devices: StorageDevice[]; physical_storage_devices: PhysicalStorageDevice[]; gpu_devices: GpuDevice[]; llama_server: string | null; nvidia_smi_available: boolean; rocm_smi_available: boolean; xpu_smi_available: boolean; inference_backend: string };

type LocalModel = { id: string; name: string; model_kind: string; format: string; root: string; relative_path: string; size_bytes: number; status: string };
type LocalWorkflow = { id: string; name: string; relative_path: string; format: string; node_count: number; node_types: string[]; status: string; execution: string };
type LocalPlugin = { id: string; name: string; display_name: string; version?: string | null; ecosystem: string; manifest_path: string; components: {agents:number;skills:number;commands:number;hooks:number;mcp_servers:number;tools:number}; status: string; trust: string; execution: string };
type AgentManifest = { id: string; name: string; adapter: string; health_url: string; version?: string | null; description?: string | null; capabilities: string[]; manifest_path: string; enabled: boolean; status: string; execution: string };
type RepairFinding = { component_id: string; severity: string; condition: string; observed_state: string; evidence: string; recommended_action: string; requires_operator_approval: boolean; auto_apply_eligible: boolean; status: string };
type RepairPlan = { status: string; checked_components: number; findings: RepairFinding[]; summary: string; safety_boundary: string };
type ComfyUiMonitor = { status: string; endpoint: string; queue: {status:string;data?:{running_count:number;pending_count:number;running:{prompt_id:string}[];pending:{prompt_id:string}[]}|null;error?:string|null}; history: {status:string;data?:{prompt_id:string;status:string;completed:boolean;outputs:{filename:string;media_type:string}[];errors:{event_type:string;message?:string|null}[]}[]|null;error?:string|null}; system: {status:string;error?:string|null} };
type Page = "overview" | "missions" | "tasks" | "agents" | "capabilities" | "models" | "gpu" | "inventory";

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
  const tone = ["online","ok","completed","approved","ready"].includes(s) ? "good" : ["running","queued","pending_approval","waiting_for_confirmation"].includes(s) ? "warm" : ["failed","offline","cancelled","degraded","locked"].includes(s) ? "bad" : "";
  return <span className={`pill ${tone}`}><i />{value.replaceAll("_"," ")}</span>;
}
function App() {
  const [token,setToken]=useState(""); const [draft,setDraft]=useState(""); const [show,setShow]=useState(false);
  const [connected,setConnected]=useState(false); const [supervisorOnline,setSupervisorConfigured]=useState(false); const [error,setError]=useState(""); const [busy,setBusy]=useState(false);
  const [models,setModels]=useState<Model[]>([]); const [model,setModel]=useState("");
  const [tasks,setTasks]=useState<Task[]>([]); const [missions,setMissions]=useState<MissionSummary[]>([]); const [agents,setAgents]=useState<Agent[]>([]); const [capabilities,setCapabilities]=useState<Capability[]>([]); const [services,setServices]=useState<Service[]>([]);
  const [selectedMission,setSelectedMission]=useState("");
  const [missionDetail,setMissionDetail]=useState<MissionDetail|null>(null);
  const [missionId,setMissionId]=useState(()=>`mission-${Date.now()}`);
  const [missionNodes,setMissionNodes]=useState(`[
  {"id":"discover","depends_on":[],"priority":5,"task_type":"service_discovery","payload":{}},
  {"id":"summarize","depends_on":["discover"],"priority":1,"task_type":"service_discovery","payload":{}}
]`);
  const [skillPreview,setSkillPreview]=useState<SkillPreview|null>(null); const [notice,setNotice]=useState(""); const [selectedSkills,setSelectedSkills]=useState<string[]>([]); const [hostProfile,setHostProfile]=useState<HostProfile|null>(null);
  const [prompt,setPrompt]=useState(""); const [active,setActive]=useState("");
  const [events,setEvents]=useState<Event[]>([]); const [result,setResult]=useState<Result|null>(null);
  const [page,setPage]=useState<Page>("overview"); const [lastRefresh,setLastRefresh]=useState<number|null>(null);
  const [localModels,setLocalModels]=useState<LocalModel[]>([]);
  const [localWorkflows,setLocalWorkflows]=useState<LocalWorkflow[]>([]);
  const [localPlugins,setLocalPlugins]=useState<LocalPlugin[]>([]);
  const [agentManifests,setAgentManifests]=useState<AgentManifest[]>([]);
  const [repairPlan,setRepairPlan]=useState<RepairPlan|null>(null);
  const inventoryScanInFlight=useRef(false);
  const [comfyMonitor,setComfyMonitor]=useState<ComfyUiMonitor|null>(null);
  const [inventoryUpdated,setInventoryUpdated]=useState<number|null>(null);
  const [inventoryBusy,setInventoryBusy]=useState(false);

  const refresh=useCallback(async()=>{
    if(!token){setConnected(false);return;}
    try{
      const [health,gpuHealth,ml,tl,mi,al,cl,sv,hp]=await Promise.all([
        api<{status:string}>("/health",""), api<{supervisor_online?:boolean}>("/api/gpu/health",token), api<{data?:Model[]}>("/api/gpu/models",token),
        api<{items?:Task[]}>("/api/tasks",token), api<{items?:MissionSummary[]}>("/api/missions",token), api<{items?:Agent[]} >("/api/agents",token), api<{items?:Capability[]}>("/api/capabilities",token), api<Service[]>("/api/services",token), api<HostProfile>("/api/system/profile",token)
      ]);
      setConnected(health.status==="ok"); setSupervisorConfigured(Boolean(gpuHealth.supervisor_online)); setModels(ml.data??[]); setTasks(tl.items??[]); setMissions(mi.items??[]); setAgents(al.items??[]); setCapabilities(cl.items??[]); setServices(sv??[]); setHostProfile(hp); setError(""); setLastRefresh(Date.now());
    }catch(e){setConnected(false);setError(e instanceof Error?e.message:"API request failed");}
  },[token]);
  useEffect(()=>{void refresh();if(!token)return;const t=window.setInterval(()=>void refresh(),5000);return()=>window.clearInterval(t);},[refresh,token]);
  useEffect(()=>{if(models.length&&!models.some(x=>x.id===model))setModel(models[0].id);},[models,model]);
  useEffect(()=>{
    if(!token||!selectedMission){setMissionDetail(null);return;}
    let dead=false;
    const load=async()=>{
      try{
        const out=await api<{item:MissionDetail}>(`/api/missions/${encodeURIComponent(selectedMission)}`,token);
        if(!dead)setMissionDetail(out.item);
      }catch(e){
        if(!dead)setError(e instanceof Error?e.message:"Mission refresh failed");
      }
    };
    void load();
    const timer=window.setInterval(()=>void load(),2500);
    return()=>{dead=true;window.clearInterval(timer);};
  },[selectedMission,token]);
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
  const refreshInventory=useCallback(async()=>{
    if(!token||inventoryScanInFlight.current)return;
    inventoryScanInFlight.current=true;
    setInventoryBusy(true);
    const [modelsResult,mediaResult,workflowsResult,pluginsResult,agentsResult,repairResult]=await Promise.allSettled([
      api<{models:LocalModel[]}>("/api/models/local",token),
      api<ComfyUiMonitor>("/api/media/comfyui/monitor",token),
      api<{workflows:LocalWorkflow[]}>("/api/media/workflows/discover",token),
      api<{plugins:LocalPlugin[]}>("/api/extensions/discover",token),
      api<{manifests:AgentManifest[]}>("/api/agents/manifests/discover",token),
      api<RepairPlan>("/api/repair/plan",token)
    ]);
    const failures:string[]=[];
    if(modelsResult.status==="fulfilled")setLocalModels(modelsResult.value.models??[]);else failures.push("local model inventory");
    if(mediaResult.status==="fulfilled")setComfyMonitor(mediaResult.value);else failures.push("ComfyUI monitor");
    if(workflowsResult.status==="fulfilled")setLocalWorkflows(workflowsResult.value.workflows??[]);else failures.push("workflow catalog");
    if(pluginsResult.status==="fulfilled")setLocalPlugins(pluginsResult.value.plugins??[]);else failures.push("plugin inventory");
    if(agentsResult.status==="fulfilled")setAgentManifests(agentsResult.value.manifests??[]);else failures.push("agent manifest inventory");
    if(repairResult.status==="fulfilled")setRepairPlan(repairResult.value);else failures.push("diagnostic repair plan");
    if(failures.length)setError("Could not refresh: "+failures.join(", "));
    else setError("");
    setInventoryUpdated(Date.now());
    setInventoryBusy(false);
    inventoryScanInFlight.current=false;
  },[token]);
  useEffect(()=>{
    if(page!=="inventory"||!token)return;
    void refreshInventory();
    const timer=window.setInterval(()=>void refreshInventory(),30000);
    return()=>window.clearInterval(timer);
  },[page,token,refreshInventory]);
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
  const planMission=async()=>{
    if(!token)return;
    setBusy(true);setError("");setNotice("");
    try{
      const nodes=JSON.parse(missionNodes) as unknown;
      if(!Array.isArray(nodes)||nodes.length===0)throw new Error("Mission nodes must be a non-empty JSON array.");
      if(!missionId.trim())throw new Error("Enter a mission ID.");
      const out=await api<{item:{mission_id:string}}>("/api/missions",token,{
        method:"POST",
        body:JSON.stringify({mission_id:missionId.trim(),nodes})
      });
      setSelectedMission(out.item.mission_id);
      setNotice("Mission " + out.item.mission_id + " was saved as a plan. No tasks were created or executed.");
      await refresh();
    }catch(e){setError(e instanceof Error?e.message:"Mission planning failed");}
    finally{setBusy(false);}
  };
  const instantiateMission=async()=>{
    if(!token||!selectedMission)return;
    setBusy(true);setError("");
    try{
      const out=await api<{item:{status:string;tasks:unknown[]}}>(
        `/api/missions/${encodeURIComponent(selectedMission)}/instantiate`,
        token,
        {method:"POST"}
      );
      setNotice("Mission instantiated with " + out.item.tasks.length + " task node(s). Every task remains approval-gated; no provider task is auto-approved.");
      const detail=await api<{item:MissionDetail}>(`/api/missions/${encodeURIComponent(selectedMission)}`,token);
      setMissionDetail(detail.item);
      await refresh();
    }catch(e){setError(e instanceof Error?e.message:"Mission instantiation failed");}
    finally{setBusy(false);}
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
      const synced=await api<{discovered:number;created:number;updated:number;unchanged:number}>(
        "/api/capabilities/refresh",
        token,
        {method:"POST",body:JSON.stringify({reason:"Operator requested local Pstack source refresh in DREAD Universal Console."})}
      );
      await refresh();
      setNotice(`Local Pstack scan: ${found.count} skills and prompt profiles · ${synced.created} added · ${synced.updated} refreshed · ${synced.unchanged} unchanged. All imports remain disabled; changed enabled or task-referenced records are refused atomically.`);
    }catch(e){setError(e instanceof Error?e.message:"Pstack source refresh failed");}
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
      const out=await api<{audit_recorded:boolean;audit_event_id:number}>(`/api/capabilities/${encodeURIComponent(skillPreview.id)}/enable`,token,{method:"POST",body:JSON.stringify({reason:"Reviewed the displayed Markdown source in DREAD Universal Console; enable as untrusted model context only."})});
      setNotice(`Enabled ${skillPreview.name} as untrusted model context. Audit event #${out.audit_event_id} was retained; no code execution is permitted.`);
      setSkillPreview(null);await refresh();
    }catch(e){setError(e instanceof Error?e.message:"Skill approval failed");}
    finally{setBusy(false);}
  };
  const disableSkill=async(item:Capability)=>{
    if(!token||!window.confirm(`Disable ${item.name} and revoke its model-context approval?`))return;
    const reason=window.prompt("Reason for disabling this capability:");
    if(reason===null||!reason.trim())return;
    setBusy(true);setError("");
    try{
      const out=await api<{audit_recorded:boolean;audit_event_id:number}>(`/api/capabilities/${encodeURIComponent(item.id)}/disable`,token,{method:"POST",body:JSON.stringify({reason})});
      setNotice(`Disabled ${item.name}. Audit event #${out.audit_event_id} was retained.`);
      setSkillPreview(null);await refresh();
    }catch(e){setError(e instanceof Error?e.message:"Skill disable failed");}
    finally{setBusy(false);}
  };
  const removeCapability=async(item:Capability)=>{
    if(!token||!window.confirm(`Remove ${item.name} from the DREAD registry? This removes only registry metadata; it does not delete files in the pinned Pstack source tree. A removal audit record will be retained.`))return;
    setBusy(true);setError("");
    try{
      const out=await api<{removed:boolean;audit_event_id:number}>(`/api/capabilities/${encodeURIComponent(item.id)}`,token,{method:"DELETE",body:JSON.stringify({reason:"Operator confirmed registry removal in DREAD Universal Console."})});
      setNotice(`Removed ${item.name} from the DREAD registry. Audit event #${out.audit_event_id} was retained; source files were not changed.`);
      if(skillPreview?.id===item.id)setSkillPreview(null);
      await refresh();
    }catch(e){setError(e instanceof Error?e.message:"Capability removal failed");}
    finally{setBusy(false);}
  };
  const disconnect=()=>{setToken("");setDraft("");setPage("overview");setActive("");setEvents([]);setResult(null);setTasks([]);setMissions([]);setSelectedMission("");setMissionDetail(null);setAgents([]);setCapabilities([]);setModels([]);setServices([]);setModel("");setLocalModels([]);setLocalWorkflows([]);setLocalPlugins([]);setAgentManifests([]);setComfyMonitor(null);setRepairPlan(null);setInventoryUpdated(null);setInventoryBusy(false);inventoryScanInFlight.current=false;setConnected(false);setSupervisorConfigured(false);};
  const activeTask=tasks.find(x=>x.id===active);
  const latestWaitEvent=[...events].reverse().find(event=>event.event_type==="agent_waiting_for_confirmation");
  const pendingActionEvent=latestWaitEvent?[...events].reverse().find(event=>event.event_type==="ActionEvent"&&event.sequence<latestWaitEvent.sequence):undefined;
  const pendingAction=pendingActionEvent?.payload??{};
  const pendingActionId=typeof pendingAction.id==="string"?pendingAction.id:"";
  const pendingActionTool=typeof pendingAction.tool_name==="string"?pendingAction.tool_name.slice(0,120):"";
  const pendingActionSummary=typeof pendingAction.summary==="string"?pendingAction.summary.slice(0,2000):"";
  const pendingActionRisk=typeof pendingAction.security_risk==="string"?pendingAction.security_risk.slice(0,32):"UNKNOWN";
  const confirmationEvents=["agent_confirmation_pending","agent_confirmation_response","agent_confirmation_uncertain","agent_confirmation_released"];
  const latestDecisionForAction=[...events].reverse().find(event=>confirmationEvents.includes(event.event_type)&&event.payload?.action_id===pendingActionId);
  const confirmationSubmissionPending=latestDecisionForAction?.event_type==="agent_confirmation_pending";
  const confirmationOutcomeUncertain=latestDecisionForAction?.event_type==="agent_confirmation_uncertain";
  const awaitingAgentConfirmation=Boolean(activeTask?.status==="running"&&latestWaitEvent&&(!latestDecisionForAction||latestWaitEvent.sequence>latestDecisionForAction.sequence||latestDecisionForAction.event_type==="agent_confirmation_released"));
  const respondToAgentConfirmation=async(accept:boolean)=>{
    if(!token||!active||!pendingActionId)return;
    setBusy(true);setError("");
    try{
      await api(`/api/tasks/${active}/agent-confirmation`,token,{method:"POST",body:JSON.stringify({
        accept,
        reason:accept?"Operator reviewed and accepted the displayed OpenHands action in DREAD Console.":"Operator rejected the displayed OpenHands action in DREAD Console.",
        expected_action_id:pendingActionId
      })});
      setNotice(accept?"OpenHands action approved. The agent may continue.":"OpenHands action rejected. The agent received the denial.");
      await refresh();
    }catch(e){setError(e instanceof Error?e.message:"OpenHands confirmation decision failed");}
    finally{setBusy(false);}
  };
  const nav:[Page,string,ReactNode,string][]=[
    ["overview","Command deck",<Activity/>,"01"],["gpu","GPU telemetry",<Gauge/>,"02"],["models","Model arsenal",<Layers3/>,"03"],["agents","Agent network",<Bot/>,"04"],["missions","Mission DAG",<Workflow/>,"05"],["tasks","Task queue",<Workflow/>,"06"],["capabilities","Pstack registry",<ShieldCheck/>,"07"],["inventory","Local inventory",<FolderSearch/>,"08"]
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
        <div className="heading"><div><h1>{page==="overview"?"Command deck":page==="missions"?"Mission orchestrator":page==="tasks"?"Task queue":page==="agents"?"Agent network":page==="capabilities"?"Pstack capability registry":page==="models"?"Model arsenal":"GPU telemetry"}</h1><p>Observe, coordinate and operate your local AI stack without crossing its safety boundaries.</p></div><div className="workstation"><span className="workstation-mark"><Cpu size={15}/></span><div><b>{hostProfile?`${hostProfile.gpu_devices.length} GPU DEVICE(S) / ${(hostProfile.distribution??hostProfile.os).toUpperCase()}`:"HOST DISCOVERY PENDING"}</b><small>{hostProfile?`${hostProfile.architecture.toUpperCase()} · ${hostProfile.cpu_physical_cores??"?"} CORES / ${hostProfile.cpu_logical_threads??"?"} LOGICAL THREADS / ${hostProfile.cpu_threads} AVAILABLE`:"LOCAL NODE / LOOPBACK"}</small></div><Pill value={connected?"LINKED":"LOCKED"}/></div></div>
        {!token&&<div className="connect"><LockKeyhole size={21}/><div><b>AUTHENTICATE COMMAND LINK</b><small>Bearer token remains in page memory only. Never persisted to browser storage.</small></div><div className="token"><KeyRound size={15}/><input type={show?"text":"password"} value={draft} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>e.key==="Enter"&&connect()} placeholder="DREAD API ACCESS TOKEN"/><button className="icon" onClick={()=>setShow(!show)}><Search size={14}/></button><button className="primary" onClick={connect}>ESTABLISH LINK</button></div></div>}
        {error&&<div className="error"><AlertCircle size={16}/>{error}<button onClick={()=>setError("")}><X size={14}/></button></div>}
        {hostProfile&&<section className="panel host-panel"><PanelTitle icon={<Cpu/>} title="AUTO-DISCOVERED HOST PROFILE" meta="READ-ONLY / NO HOST MUTATIONS"/><div className="host-grid"><div><small>UBUNTU / DISTRIBUTION</small><b>{hostProfile.distribution??hostProfile.os} {hostProfile.distribution_version??""} · {hostProfile.architecture}</b></div><div><small>KERNEL</small><b>{hostProfile.kernel_release??"NOT REPORTED"}</b></div><div><small>CPU MODEL</small><b>{hostProfile.cpu_model??"NOT REPORTED"}</b></div><div><small>CPU PHYSICAL CORES / LOGICAL THREADS / AVAILABLE</small><b>{hostProfile.cpu_physical_cores??"N/A"} / {hostProfile.cpu_logical_threads??"N/A"} / {hostProfile.cpu_threads}</b></div><div><small>CPU SOCKET / CORE / NUMA TOPOLOGY</small><b>{hostProfile.cpu_sockets??"?"} SOCKET(S) · {hostProfile.cpu_cores_per_socket??"?"} CORE(S)/SOCKET · {hostProfile.cpu_threads_per_core??"?"} THREAD(S)/CORE · {hostProfile.cpu_numa_nodes??"?"} NUMA NODE(S)</b></div><div><small>NVIDIA DRIVER CUDA COMPATIBILITY</small><b>{hostProfile.cuda_version??"NOT REPORTED"}</b></div><div><small>CUDA TOOLKITS / COMPILERS</small><b>{hostProfile.cuda_toolkits.length?hostProfile.cuda_toolkits.map(toolkit=>`${toolkit.version} · ${toolkit.compiler_path}`).join(" | "):hostProfile.cuda_toolkit_version??"NOT FOUND"}</b></div><div><small>RAM TOTAL / AVAILABLE</small><b>{mib(hostProfile.memory_total_mib)} / {mib(hostProfile.memory_available_mib)}</b></div><div><small>ROOT DISK TOTAL / FREE</small><b>{mib(hostProfile.root_disk_total_mib)} / {mib(hostProfile.root_disk_available_mib)}</b></div><div><small>SELECTED INFERENCE BACKEND</small><b>{hostProfile.inference_backend}</b></div><div><small>LLAMA-SERVER EXECUTABLE</small><b>{hostProfile.llama_server??"NOT DISCOVERED (SUPERVISOR MAY LAUNCH ON DEMAND)"}</b></div></div><div className="host-gpus">{hostProfile.gpu_devices.length?hostProfile.gpu_devices.map((gpu,index)=><div className="row" key={`${gpu.device_id??gpu.pci_bus_id??gpu.vendor}-${index}`}><div><b>{gpu.name}</b><small>{gpu.vendor.toUpperCase()} · {gpu.vram_used_mib===null?"VRAM USE N/A":`${gpu.vram_used_mib.toLocaleString()}/${gpu.vram_mib?.toLocaleString()??"?"} MiB · ${gpu.vram_free_mib?.toLocaleString()??"?"} MiB FREE`} · {gpu.driver_version?`DRIVER ${gpu.driver_version}`:"DRIVER N/A"} · {gpu.compute_capability?`COMPUTE ${gpu.compute_capability}`:"COMPUTE N/A"} · {gpu.pcie_link_gen?`PCIe CURRENT Gen${gpu.pcie_link_gen} x${gpu.pcie_link_width??"?"} / MAX Gen${gpu.pcie_link_gen_max??"?"} x${gpu.pcie_link_width_max??"?"}`:"PCIe N/A"} · {gpu.utilization_percent===null?"LOAD N/A":`${gpu.utilization_percent}% LOAD`} · {gpu.temperature_c===null?"TEMP N/A":`${gpu.temperature_c}°C`} · {gpu.power_draw_mw===null?"POWER N/A":`${(gpu.power_draw_mw/1000).toFixed(1)} W`}</small></div><Pill value="DETECTED"/></div>):<Empty text="No supported GPU utility reported a device. CPU-only host remains discoverable."/>}</div><div className="host-storage"><div className="storage-title"><small>MOUNTED PERSISTENT / NETWORK FILESYSTEMS</small><b>{hostProfile.storage_devices.length} MOUNT(S)</b></div>{hostProfile.storage_devices.length?hostProfile.storage_devices.map((disk,index)=><div className="row" key={`${disk.source}-${disk.mount_point}-${index}`}><div><b>{disk.mount_point}</b><small>{disk.source} · {disk.filesystem.toUpperCase()} · {mib(disk.used_mib)} USED / {mib(disk.total_mib)} TOTAL · {mib(disk.available_mib)} FREE</small></div><Pill value="MOUNTED"/></div>):<Empty text="No persistent or network mounts reported by df."/>}</div><div className="host-storage"><div className="storage-title"><small>PHYSICAL DISKS / PARTITIONS (MOUNTED OR UNMOUNTED)</small><b>{hostProfile.physical_storage_devices.length} DISK(S)</b></div>{hostProfile.physical_storage_devices.length?hostProfile.physical_storage_devices.map((disk,index)=><div className="row" key={`${disk.name}-${index}`}><div><b>{disk.name} · {disk.model??"MODEL N/A"}</b><small>{disk.transport?.toUpperCase()??"TRANSPORT N/A"} · {mib(disk.size_mib)} · {disk.partitions.length?disk.partitions.map(part=>`${part.name} ${part.filesystem??"NO FS"} ${part.mount_points.join(", ")||"UNMOUNTED"}`).join(" | "):"NO PARTITIONS REPORTED"}</small></div><Pill value="PHYSICAL DISK"/></div>):<Empty text="No physical block devices reported by lsblk."/>}</div><div className="host-tools"><span>NVIDIA-SMI {hostProfile.nvidia_smi_available?"FOUND":"MISSING"}</span><span>ROCM-SMI {hostProfile.rocm_smi_available?"FOUND":"MISSING"}</span><span>XPU-SMI {hostProfile.xpu_smi_available?"FOUND":"MISSING"}</span><span>PCIe values are a point-in-time snapshot; idle Gen1 downshift is expected. Validate CURRENT vs MAX only under sustained GPU load.</span></div></section>}
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
          <section className="panel gpu-strip"><PanelTitle icon={<Gauge/>} title="DETECTED GPU ARRAY" meta="HOST PROFILE / TELEMETRY OPTIONAL"/><div className="gpu-pair">{hostProfile?.gpu_devices.length?hostProfile.gpu_devices.map((gpu,index)=><GpuCard key={`${gpu.vendor}-${index}`} index={`GPU ${gpu.device_index??index}`} gpu={gpu}/>):<Empty text="No supported GPU detected. The control plane remains available for CPU-only llama.cpp when configured."/>}</div><div className="telemetry-note"><Activity size={13}/> Read-only hardware samples come from host discovery. The Python GPU Supervisor separately owns inference admission and process lifecycle.</div><button className="link" onClick={()=>setPage("gpu")}>OPEN GPU TELEMETRY <ChevronRight size={14}/></button></section>
          <section className="panel system-panel"><PanelTitle icon={<Workflow/>} title="SYSTEM SYNAPSE" meta="LIVE SERVICE MAP"/><div className="synapse"><div className="syn-node hub-node"><span className="node-core"><Activity size={18}/></span><b>RUST CORE</b><small>{connected?"API CONNECTED":"AWAITING LINK"}</small></div><div className="syn-branches"><div className="syn-branch"><span className="syn-wire"/><div className="syn-node"><Cpu size={17}/><b>GPU SUPERVISOR</b><small>{supervisorOnline?"AUTHENTICATED":"LOCKED / :11502"}</small></div></div><div className="syn-branch"><span className="syn-wire"/><div className="syn-node"><Server size={17}/><b>COMFYUI RUNTIME</b><small>{comfy?.status??"NOT DISCOVERED"}</small></div></div><div className="syn-branch"><span className="syn-wire"/><div className="syn-node"><Bot size={17}/><b>AGENT FABRIC</b><small>{agents.length} CONFIGURED / READ ONLY</small></div></div></div></div><div className="system-foot"><span><i className="legend-online"/>ONLINE</span><span><i className="legend-locked"/>LOCKED / NOT CONFIGURED</span><span><i className="legend-line"/>DISCOVERY LINK</span></div></section>
          <section className="panel queue-panel"><PanelTitle icon={<Workflow/>} title="MISSION QUEUE" meta="5 SEC REFRESH"/><TaskList tasks={tasks.slice(0,5)} active={active} select={setActive} action={action} busy={busy}/><button className="link" onClick={()=>setPage("tasks")}>OPEN MISSION CONTROL <ChevronRight size={14}/></button></section>
          <section className="panel service-panel"><PanelTitle icon={<Radio/>} title="LOCAL SERVICE MATRIX" meta="READ-ONLY"/>{services.length?services.map(x=><div className="row" key={x.id}><div><b>{x.name}</b><small>{x.role} · {x.latency_ms??"—"} ms</small></div><Pill value={x.status}/></div>):<Empty text="Connect to load configured local services."/>}</section>
          <section className="panel agent-panel"><PanelTitle icon={<Bot/>} title="AGENT FABRIC" meta="ALLOWLISTED LOCAL / EXPLICIT"/>{agents.length?agents.map(x=><div className="row" key={x.id}><div><b>{x.name}</b><small>{x.adapter??x.id}</small></div><Pill value={x.status}/></div>):<Empty text="No known local agent endpoint responded. DREAD probes only allowlisted loopback health URLs; it does not sweep ports or launch agents."/>}<button className="link" onClick={()=>setPage("agents")}>INSPECT AGENT NETWORK <ChevronRight size={14}/></button></section>
        </div>}
        {page==="gpu"&&<><div className="section-head"><div><label>HARDWARE / RESOURCE DOMAIN</label><h2>GPU telemetry matrix</h2><p>Read-only host samples are displayed independently of inference lease availability.</p></div><Pill value={supervisorOnline?"SUPERVISOR ONLINE":"SUPERVISOR OFFLINE"}/></div><div className="gpu-pair large">{hostProfile?.gpu_devices.length?hostProfile.gpu_devices.map((gpu,index)=><GpuCard key={`${gpu.vendor}-${index}`} index={`GPU ${index}`} gpu={gpu}/>):<Empty text="No supported GPU detected. CPU inference remains available when a local llama.cpp runtime is configured."/>}</div><div className="telemetry-note"><Activity size={13}/> Hardware telemetry is sampled by the host discovery layer. Supervisor status controls inference leases, not whether read-only GPU metrics may be displayed.</div></>}
        {page==="models"&&<section className="panel"><PanelTitle icon={<Layers3/>} title="LOCAL MODEL ARSENAL" meta={`${models.length} DISCOVERED`}/>{models.length?models.map(x=><div className="row" key={x.id}><div><b>{x.id}</b><small>Discovered from local GPU gateway</small></div><Pill value="AVAILABLE"/></div>):<Empty text="No models returned by the local GPU gateway. Confirm gateway and upstream model service status."/>}</section>}

        {page==="inventory"&&<div className="command-grid">
          <section className="panel">
            <div className="panel-toolbar"><PanelTitle icon={<FolderSearch/>} title="LOCAL MODEL FILES" meta={String(localModels.length)+" CANDIDATES"}/><button className="secondary" disabled={!token||inventoryBusy} onClick={()=>void refreshInventory()}><RefreshCw size={14}/>REFRESH INVENTORY</button></div>
            <p className="muted">Filesystem discovery only. These are candidate weight files, not proof of model compatibility or loaded state. Loading and unloading remain under the Python GPU Supervisor.</p>
            {localModels.length?localModels.slice(0,100).map(item=><div className="row" key={item.id}><div><b>{item.name}</b><small>{item.model_kind} · {item.format.toUpperCase()} · {item.relative_path}</small></div><div className="task-actions"><Pill value="discovered_not_loaded"/><small>{(item.size_bytes/1073741824).toFixed(2)} GiB</small></div></div>):<Empty text="No local model files found under configured model roots. Set DREAD_MODEL_ROOTS to scan additional directories."/>}
            {localModels.length>100&&<p className="muted">Showing first 100 of {localModels.length} discovered files.</p>}
          </section>
          <section className="panel">
            <PanelTitle icon={<Workflow/>} title="COMFYUI MEDIA RUNTIME" meta={comfyMonitor?.status?.toUpperCase()??"NOT CHECKED"}/>
            {comfyMonitor?<><div className="row"><div><b>Queue running</b><small>{comfyMonitor.queue.error??comfyMonitor.endpoint}</small></div><Pill value={String(comfyMonitor.queue.data?.running_count??"unavailable")}/></div><div className="row"><div><b>Queue pending</b><small>Live queue metadata only</small></div><Pill value={String(comfyMonitor.queue.data?.pending_count??"unavailable")}/></div><div className="row"><div><b>Recent history</b><small>{comfyMonitor.history.error??"Prompt IDs, completion status and output metadata only"}</small></div><Pill value={String(comfyMonitor.history.data?.length??"unavailable")}/></div>{comfyMonitor.history.data?.slice(0,5).map(job=><div className="row" key={job.prompt_id}><div><b>{job.prompt_id.slice(0,16)}</b><small>{job.status} · {job.outputs.length} output(s)</small></div><Pill value={job.completed?"completed":job.status}/></div>)}</>:<Empty text="ComfyUI has not been queried yet. Open this page to run the read-only monitor."/>}
            <div className="panel-toolbar"><PanelTitle icon={<Layers3/>} title="WORKFLOW CATALOG" meta={String(localWorkflows.length)+" FOUND"}/></div>
            <p className="muted">Workflow JSON is parsed for node metadata only. Payloads, widget values and credentials are not returned, and discovery never submits a workflow.</p>
            {localWorkflows.length?localWorkflows.slice(0,60).map(item=><div className="row" key={item.id}><div><b>{item.name}</b><small>{item.format} · {item.node_count} nodes · {item.relative_path}</small><small>{item.node_types.slice(0,6).join(", ")}</small></div><Pill value="not validated"/></div>):<Empty text="No supported ComfyUI workflow JSON files were discovered under the configured workflow roots."/>}
            {localWorkflows.length>60&&<p className="muted">Showing first 60 of {localWorkflows.length} workflow files.</p>}
          </section>
          <section className="panel">
            <PanelTitle icon={<ShieldCheck/>} title="LOCAL PLUGIN MANIFESTS" meta={String(localPlugins.length)+" DISCOVERED"}/>
            <p className="muted">Cursor, Claude and generic manifests are inventory metadata only. Discovered plugins are untrusted and disabled; hooks, scripts, tools and MCP servers are never started.</p>
            {localPlugins.length?localPlugins.slice(0,100).map(item=><div className="row" key={item.id}><div><b>{item.display_name}</b><small>{item.ecosystem} · {item.name}{item.version?" · "+item.version:""}</small><small>{item.manifest_path} · {item.components.skills} skills / {item.components.tools} tools / {item.components.mcp_servers} MCP servers</small></div><div className="task-actions"><Pill value="untrusted"/><Pill value="disabled"/></div></div>):<Empty text="No local plugin manifests found under the configured Cursor / Claude / generic roots."/>}
            {localPlugins.length>100&&<p className="muted">Showing first 100 of {localPlugins.length} plugin manifests.</p>}
            <div className="panel-toolbar"><PanelTitle icon={<Bot/>} title="LOCAL AGENT ADAPTER MANIFESTS" meta={String(agentManifests.length)+" CANDIDATES"}/></div>
            <p className="muted">These manifests are discovered automatically from configured and standard per-user locations. DREAD does not probe the declared URL or start an agent during discovery. A candidate must be reviewed, mapped to a supported adapter, explicitly configured, and separately enabled before health checks or execution.</p>
            {agentManifests.length?agentManifests.slice(0,100).map(item=><div className="row" key={item.manifest_path}><div><b>{item.name}</b><small>{item.id} · adapter {item.adapter}{item.version?" · "+item.version:""}</small><small>{item.health_url} · {item.manifest_path}</small><small>{item.capabilities.slice(0,8).join(", ")||"No capability metadata"}{item.description?" · "+item.description:""}</small></div><div className="task-actions"><Pill value="review_required"/><Pill value="disabled"/></div></div>):<Empty text="No local agent manifests found. Configure DREAD_AGENT_MANIFEST_ROOTS or add agent.toml / *.agent.toml files under config/agents.d or the per-user DREAD config directory."/>}
            {agentManifests.length>100&&<p className="muted">Showing first 100 of {agentManifests.length} agent manifests.</p>}
            <p className="muted">{inventoryUpdated?"Last scan "+new Date(inventoryUpdated).toLocaleTimeString()+".":"Inventory has not been scanned."} Automatic refresh runs every 30 seconds while this page is open.</p>
          </section>
          <section className="panel">
            <PanelTitle icon={<ShieldCheck/>} title="SELF-REPAIR DIAGNOSTICS" meta={repairPlan?.status?.toUpperCase()??"NOT CHECKED"}/>
            <p className="muted">This is the diagnosis and proposal layer only. It does not edit files, restart services, load/unload models, alter GPU leases or apply repairs.</p>
            {repairPlan?<><div className="row"><div><b>{repairPlan.summary}</b><small>{repairPlan.checked_components} component(s) observed</small></div><Pill value="diagnostic_plan_only"/></div>
              {repairPlan.findings.length?repairPlan.findings.map(item=><div className="row" key={item.component_id+item.condition}><div><b>{item.component_id} · {item.condition.replaceAll("_"," ")}</b><small>{item.evidence}</small><small>Proposal: {item.recommended_action}</small></div><div className="task-actions"><Pill value={item.severity}/><Pill value="proposed_not_applied"/></div></div>):<Empty text="No repair findings were produced from the currently observable components."/>}
              <p className="muted">{repairPlan.safety_boundary}</p>
            </>:<Empty text="Diagnostic plan has not been loaded."/>}
          </section>
          <section className="panel">
            <div className="row"><div><b>Inventory refresh guard</b><small>Disk scans are serialized; one refresh runs at a time.</small></div><Pill value={inventoryBusy?"running":"idle"}/></div>
          </section>
        </div>}
        {page==="missions"&&<div className="task-layout">
          <section className="panel">
            <PanelTitle icon={<Workflow/>} title="MISSION DAG PLANNER" meta="PLAN FIRST / APPROVAL LATER"/>
            <p className="muted">Create a dependency graph. Planning persists the graph only; it does not create or run tasks. Review the plan, then instantiate it separately.</p>
            <label>MISSION ID</label>
            <textarea rows={1} value={missionId} onChange={e=>setMissionId(e.target.value)} disabled={!token||busy}/>
            <label>MISSION NODES · JSON ARRAY</label>
            <textarea rows={12} value={missionNodes} onChange={e=>setMissionNodes(e.target.value)} spellCheck={false} disabled={!token||busy}/>
            <div className="compose-foot">
              <small>NODE TYPES: service_discovery · gpu_chat · agent_task</small>
              <button className="primary" disabled={!token||busy||!missionId.trim()} onClick={()=>void planMission()}><Plus size={14}/>PLAN MISSION</button>
            </div>
            <p className="muted">For agent_task nodes, payload must include an approved capability_id and an input object. Supported provider IDs are hermes.run, deerflow.run and openhands.run. All content is validated by the API.</p>
            {notice&&<div className="approval">{notice}</div>}
          </section>
          <section className="panel">
            <PanelTitle icon={<Layers3/>} title="PERSISTED MISSION REGISTRY" meta={String(missions.length)+" / LATEST 100"}/>
            {missions.length?missions.map(mission=><div className="row" key={mission.mission_id}>
              <button className="task-name" onClick={()=>setSelectedMission(mission.mission_id)}>
                <span><Workflow size={15}/></span>
                <div><b>{mission.mission_id}</b><small>{mission.node_count} NODE(S) · {clock(mission.created_at)}</small></div>
              </button>
              <div className="task-actions"><Pill value={mission.status}/></div>
            </div>):<Empty text="No persisted mission plans. Create a DAG on the left."/>}
            {missionDetail&&<div className="skill-preview">
              <div className="row"><div><b>{missionDetail.mission_id}</b><small>Execution order: {missionDetail.execution_order.join(" → ")}</small></div><Pill value={missionDetail.status}/></div>
              {missionDetail.status==="planned"&&<div className="compose-foot">
                <small>Instantiate creates approval-gated task records only.</small>
                <button className="primary" disabled={busy} onClick={()=>void instantiateMission()}><Play size={14}/>INSTANTIATE FOR APPROVAL</button>
              </div>}
              {missionDetail.tasks.length?missionDetail.tasks.map(item=><div className="row" key={item.task_id}>
                <div><b>{item.node_id}</b><small>{item.depends_on.length?"Depends on: "+item.depends_on.join(", "):"No dependencies"} · {item.attempts} attempt(s)</small></div>
                <div className="task-actions"><Pill value={item.status}/><button className="link" onClick={()=>{setActive(item.task_id);setPage("tasks");}}>VIEW TASK</button></div>
              </div>):<Empty text="This graph has no instantiated tasks yet. Planning alone cannot execute work."/>}
            </div>}
          </section>
        </div>}
        {page==="tasks"&&<div className="task-layout"><section className="panel"><PanelTitle icon={<Workflow/>} title="TASK QUEUE" meta={`${tasks.length} RECORDED`}/><TaskList tasks={tasks} active={active} select={setActive} action={action} busy={busy}/><button className="secondary full" disabled={!token||busy} onClick={()=>void create("service_discovery")}><Plus size={14}/>CREATE SERVICE DISCOVERY</button></section><section className="panel"><PanelTitle icon={<TerminalSquare/>} title="MISSION TRACE" meta={active?active.slice(0,8).toUpperCase():"SELECT MISSION"}/>{activeTask?<><div className="detail"><b>{activeTask.title}</b><small>{activeTask.task_type} · {activeTask.id}</small><Pill value={activeTask.status}/></div>
        {awaitingAgentConfirmation&&<div className="agent-confirmation">
          <div className="agent-confirmation-head"><div><b>OPENHANDS ACTION WAITING FOR APPROVAL</b><small>Review the provider summary. Command arguments and model reasoning are deliberately hidden.</small></div><Pill value="waiting_for_confirmation"/></div>
          <div className="agent-confirmation-grid">
            <div><small>REQUESTED TOOL</small><b>{pendingActionTool||"Unknown tool"}</b></div>
            <div><small>PROVIDER RISK LABEL</small><Pill value={pendingActionRisk}/></div>
          </div>
          <div className="agent-confirmation-summary"><small>PROVIDER SUMMARY · UNTRUSTED TEXT</small><p>{pendingActionSummary||"No safe action summary was supplied. Approval is disabled."}</p></div>
          <div className="agent-confirmation-actions">
            <button className="primary" disabled={!token||busy||!pendingActionId||!pendingActionTool||!pendingActionSummary} onClick={()=>void respondToAgentConfirmation(true)}><Check size={14}/>APPROVE ACTION</button>
            <button className="secondary" disabled={!token||busy||!pendingActionId} onClick={()=>void respondToAgentConfirmation(false)}><X size={14}/>REJECT ACTION</button>
          </div>
          {!pendingActionId&&<p className="agent-confirmation-warning">The provider action identifier is missing. Refresh the trace; decisions remain disabled.</p>}
        </div>}
        {activeTask.status==="running"&&confirmationSubmissionPending&&<div className="agent-confirmation-status pending"><b>OPENHANDS DECISION SUBMITTED</b><p>DREAD has reserved this action and sent the decision to the provider. Wait for the provider state to update; do not retry.</p></div>}
        {activeTask.status==="running"&&confirmationOutcomeUncertain&&<div className="agent-confirmation-status uncertain"><b>OPENHANDS DECISION OUTCOME UNCERTAIN</b><p>The provider did not confirm receipt. This action is locked against duplicate submission. Inspect the provider session or cancel the task; do not retry.</p></div>}
        <div className="events">{events.map(x=><div className="event" key={x.sequence}><code>#{x.sequence}</code><span>{x.event_type.replaceAll("_"," ")}</span><small>{clock(x.created_at)}</small></div>)}{!events.length&&<Empty text="No mission events yet."/>}</div>{result&&<div className="result"><b>EXECUTION RESULT</b><pre>{JSON.stringify(result.result??result.error??result,null,2)}</pre></div>}</>:<Empty text="Select a mission to inspect its audit events and result."/>}</section></div>}
        {page==="agents"&&<section className="panel"><PanelTitle icon={<Bot/>} title="AGENT NETWORK" meta="READ-ONLY HEALTH PROBES"/>{agents.length?agents.map(x=><div className="row" key={x.id}><div><b>{x.name}</b><small>{x.adapter??"generic"} · {x.id}</small></div><Pill value={x.status}/></div>):<Empty text="Agent inventory is empty. Add reviewed loopback health endpoints to config/agents.toml. Discovery never scans ports or launches agents."/>}</section>}
        {page==="capabilities"&&<section className="panel"><div className="panel-toolbar"><PanelTitle icon={<ShieldCheck/>} title="PSTACK CAPABILITY INVENTORY" meta="DISCOVER / PREVIEW / APPROVE"/><button className="primary" disabled={!token||busy} onClick={()=>void discoverSkills()}><Plus size={14}/>SYNC / REFRESH LOCAL CONTEXT</button></div><p className="muted">Sync local Markdown skills and prompt profiles as disabled, untrusted context. SHA-256 fingerprints detect local content changes but do not prove publisher identity. Changed records refresh only while disabled and unreferenced; one active or task-referenced change aborts the full transaction. No imported code executes.</p>{notice&&<div className="approval">{notice}</div>}{skillPreview&&<div className="skill-preview"><div className="row"><div><b>REVIEW SOURCE · {skillPreview.name}</b><small>{skillPreview.id} · {skillPreview.trust} · execution: {skillPreview.execution}</small></div><button className="icon" onClick={()=>setSkillPreview(null)}><X size={14}/></button></div><p className="muted">{skillPreview.description}</p><pre className="result">{skillPreview.content}</pre><div className="compose-foot"><small>Read the full source before approving this skill.</small><button className="primary" disabled={busy} onClick={()=>void enableSkill()}><ShieldCheck size={14}/>APPROVE AS MODEL CONTEXT</button></div></div>}{capabilities.length?capabilities.map(item=><div className="row capability-row" key={item.id}><div className="capability-main"><b>{item.name}</b><small>{item.kind.replaceAll("_"," ")} · {item.id}</small><small>{item.description||"No description supplied."}</small><small>Source: {item.provenance.source_name}{item.provenance.source_version?` · ${item.provenance.source_version}`:""}{item.version?` · Capability ${item.version}`:""}</small><small>Compatibility: {item.compatibility.replaceAll("_"," ")} · Dependencies: {item.dependencies?.length?item.dependencies.join(", "):"none listed"}{item.adapter?` · Adapter: ${item.adapter}`:""}</small></div><div className="capability-state"><Pill value={item.status}/>{item.license&&<small>{item.license}</small>}{(item.kind==="skill"||item.kind==="agent")&&<div className="capability-actions"><button className="link" disabled={busy} onClick={()=>void previewSkill(item)}>PREVIEW</button>{item.status==="enabled"?<button className="link" disabled={busy} onClick={()=>void disableSkill(item)}>DISABLE</button>:<><small>{item.status==="rejected"?"Rejected · removal only":"Approval required"}</small>{(item.status==="disabled"||item.status==="rejected")&&<button className="link" disabled={busy} onClick={()=>void removeCapability(item)}>REMOVE</button>}</>}</div>}</div></div>):<Empty text="No capabilities imported. Use Discover & Import Skills to scan the configured Pstack catalog."/>}</section>}
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
  return <div className="gpu-card"><div className="gpu-card-top"><div className="gpu-chip"><Cpu size={20}/></div><div><small>{index} · {gpu.vendor.toUpperCase()}</small><b>{gpu.name}</b><small>{gpu.driver_version? `DRIVER ${gpu.driver_version}`:"DRIVER N/A"} · {pcie}</small><small>{gpu.device_id?`ID ${gpu.device_id.slice(0,20)}`:"ID N/A"} · {gpu.pci_bus_id??"PCI BUS N/A"}</small></div><Pill value={hasTelemetry?"TELEMETRY AVAILABLE":"DETECTED / N/A"}/></div><div className="gpu-scope"><div className="scope-grid"/><div className="scope-scan"/><div className="scope-center"><Activity size={19}/><b>{utilization}</b><small>GPU UTILIZATION</small></div></div><div className="gpu-readings"><span><small>UTILIZATION</small><b>{utilization}</b></span><span><small>VRAM USED / TOTAL</small><b>{vram}</b></span><span><small>TEMPERATURE</small><b>{temperature}</b></span><span><small>POWER DRAW</small><b>{power}</b></span></div></div>;
}
function TaskList({tasks,active,select,action,busy}:{tasks:Task[];active:string;select:(id:string)=>void;action:(task:Task,verb:"approve"|"run"|"cancel")=>void;busy:boolean}){
  if(!tasks.length)return <Empty text="No missions have been created." />;
  return <div>{tasks.map(task=><div className={active===task.id?"task selected":"task"} key={task.id}><button className="task-name" onClick={()=>select(task.id)}><span>{task.task_type==="gpu_chat"?<MessageSquare size={15}/>:<Search size={15}/>}</span><div><b>{task.title}</b><small>{task.task_type} · {clock(task.created_at)}</small></div></button><div className="task-actions"><Pill value={task.status}/>{task.status==="pending_approval"&&<button title="Approve" disabled={busy} onClick={()=>action(task,"approve")}><Check size={14}/></button>}{task.status==="approved"&&<button title="Run" disabled={busy} onClick={()=>action(task,"run")}><Play size={14}/></button>}{["queued","running"].includes(task.status)&&<button title="Cancel" disabled={busy} onClick={()=>action(task,"cancel")}><X size={14}/></button>}</div></div>)}</div>;
}
export default App;
