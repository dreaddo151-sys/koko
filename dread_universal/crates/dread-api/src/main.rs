mod store;
use axum::{body::Body,extract::{Path,State},http::{header,HeaderValue,StatusCode},response::{IntoResponse,Response},routing::{get,post},Json,Router};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{env,sync::Arc,time::Duration};
use store::Store;

#[derive(Clone)] struct AppState{http:reqwest::Client,llama_url:String,comfy_url:String,model:String,store:Store}
#[derive(serde::Serialize)] struct Health{status:&'static str,inference_backend:&'static str,inference_url:String}
#[derive(Deserialize)] struct ChatRequest{messages:Vec<Value>,#[serde(default)]model:Option<String>,#[serde(default)]stream:bool,#[serde(default)]conversation_id:Option<String>,#[serde(flatten)]options:std::collections::HashMap<String,Value>}
async fn health(State(s):State<Arc<AppState>>)->Json<Health>{Json(Health{status:"ok",inference_backend:"llama.cpp",inference_url:s.llama_url.clone()})}
async fn services(State(s):State<Arc<AppState>>)->Json<Value>{let mut out=Vec::new();for(id,url)in[("llama_cpp",format!("{}/health",s.llama_url)),("comfyui",format!("{}/system_stats",s.comfy_url))]{let v=match s.http.get(&url).send().await{Ok(r)=>json!({"id":id,"url":url,"status":if r.status().is_success(){"ONLINE"}else{"DEGRADED"},"http_status":r.status().as_u16()}),Err(e)=>json!({"id":id,"url":url,"status":"OFFLINE","detail":e.to_string()})};out.push(v);}Json(json!({"services":out,"gpu_authority":"external-python-supervisor"}))}
async fn models(State(s):State<Arc<AppState>>)->Result<Json<Value>,(StatusCode,String)>{let r=s.http.get(format!("{}/v1/models",s.llama_url)).send().await.map_err(upstream)?;let status=r.status();let body=r.json::<Value>().await.map_err(upstream)?;if !status.is_success(){return Err((StatusCode::BAD_GATEWAY,body.to_string()));}Ok(Json(body))}
fn content(v:&Value)->String{match v.get("content"){Some(Value::String(x))=>x.clone(),Some(x)=>x.to_string(),None=>String::new()}}
async fn chat(State(s):State<Arc<AppState>>,Json(mut q):Json<ChatRequest>)->Result<Response,(StatusCode,String)>{
 if q.messages.is_empty(){return Err((StatusCode::BAD_REQUEST,"messages must not be empty".into()));}
 let model=q.model.take().unwrap_or(s.model.clone());let conversation=q.conversation_id.take().unwrap_or_else(||format!("dread-{}",uuid::Uuid::new_v4()));
 let user=q.messages.iter().rev().find(|m|m["role"]=="user").map(content).unwrap_or_default();
 let mut payload=json!({"messages":q.messages,"model":model,"stream":q.stream});
 if q.stream{payload["stream_options"]=json!({"include_usage":true});}
 if let Some(map)=payload.as_object_mut(){for(k,v)in q.options{if !["messages","model","stream","stream_options"].contains(&k.as_str()){map.insert(k,v);}}}
 let upstream_response=s.http.post(format!("{}/v1/chat/completions",s.llama_url)).json(&payload).send().await.map_err(upstream)?;
 if !upstream_response.status().is_success(){let body=upstream_response.text().await.unwrap_or_default();return Err((StatusCode::BAD_GATEWAY,body));}
 if q.stream{let mut r=Response::new(Body::from_stream(upstream_response.bytes_stream()));*r.status_mut()=StatusCode::OK;r.headers_mut().insert(header::CONTENT_TYPE,HeaderValue::from_static("text/event-stream"));r.headers_mut().insert(header::CACHE_CONTROL,HeaderValue::from_static("no-cache"));r.headers_mut().insert("x-dread-conversation-id",HeaderValue::from_str(&conversation).unwrap_or_else(|_|HeaderValue::from_static("unavailable")));return Ok(r);}
 let body=upstream_response.json::<Value>().await.map_err(upstream)?;let usage=&body["usage"];let p=usage["prompt_tokens"].as_i64().unwrap_or(0);let c=usage["completion_tokens"].as_i64().unwrap_or(0);let t=usage["total_tokens"].as_i64().unwrap_or(p+c);let answer=body["choices"][0]["message"]["content"].as_str().unwrap_or("");
 s.store.save_exchange(&conversation,&model,&user,answer,(p,c,t)).map_err(internal)?;
 let mut r=Json(json!({"conversation_id":conversation,"response":body})).into_response();r.headers_mut().insert("x-dread-conversation-id",HeaderValue::from_str(&conversation).unwrap_or_else(|_|HeaderValue::from_static("unavailable")));Ok(r)
}

#[derive(Deserialize)] struct TokenizeRequest { content:String }
async fn tokenize_text(s:&AppState,text:&str)->Result<usize,(StatusCode,String)> {
 let r=s.http.post(format!("{}/tokenize",s.llama_url)).json(&json!({"content":text})).send().await.map_err(upstream)?;
 if !r.status().is_success(){return Err((StatusCode::BAD_GATEWAY,"llama.cpp tokenizer endpoint failed; no approximate count substituted".into()));}
 let v=r.json::<Value>().await.map_err(upstream)?;
 let count=v["tokens"].as_array().map(|a|a.len()).or_else(||v.as_array().map(|a|a.len()))
  .ok_or_else(||(StatusCode::BAD_GATEWAY,"unexpected llama.cpp /tokenize response".into()))?;
 Ok(count)
}
async fn tokenize(State(s):State<Arc<AppState>>,Json(q):Json<TokenizeRequest>)->Result<Json<Value>,(StatusCode,String)> {
 let n=tokenize_text(&s,&q.content).await?;Ok(Json(json!({"tokens":n,"tokenizer":"llama.cpp","approximate":false})))
}
#[derive(Deserialize)] struct BoostRequest { prompt:String, #[serde(default)] max_tokens:Option<usize> }
async fn boost_prompt(State(s):State<Arc<AppState>>,Json(q):Json<BoostRequest>)->Result<Json<Value>,(StatusCode,String)> {
 let original_tokens=tokenize_text(&s,&q.prompt).await?;
 // Conservative formatting cleanup: preserve leading indentation and line structure.
 // Trailing spaces are removed; Markdown hard-break semantics may change, so preview first.
 let normalized=q.prompt.lines().map(str::trim_end).collect::<Vec<_>>().join("\n");
 let boosted_tokens=tokenize_text(&s,&normalized).await?;
 let budget=q.max_tokens.unwrap_or(usize::MAX);
 Ok(Json(json!({"original_prompt":q.prompt,"boosted_prompt":normalized,
  "original_tokens":original_tokens,"boosted_tokens":boosted_tokens,
  "tokens_saved":original_tokens.saturating_sub(boosted_tokens),
  "budget":if budget==usize::MAX {Value::Null}else{json!(budget)},
  "within_budget":boosted_tokens<=budget,"method":"trailing_whitespace_cleanup",
  "semantic_compression":false,"requires_user_review":true,"note":"Leading indentation and line breaks are preserved. Trailing-space Markdown hard breaks may change; inspect before applying. No semantic compression is performed."})))
}

#[derive(Deserialize)] struct OptimizeRequest { prompt:String, #[serde(default)] max_tokens:Option<usize> }
async fn optimize_prompt(State(s):State<Arc<AppState>>,Json(q):Json<OptimizeRequest>)->Result<Json<Value>,(StatusCode,String)> {
 if q.prompt.trim().is_empty(){return Err((StatusCode::BAD_REQUEST,"prompt must not be empty".into()));}
 let before=tokenize_text(&s,&q.prompt).await?;
 let instructions="You are a prompt editor. Rewrite the user's prompt to be clearer, more structured, and easier for an AI model to follow. Preserve every factual detail, constraint, safety requirement, named entity, requested output, and priority. Do not answer or execute the prompt. Do not invent requirements or remove ambiguity silently; retain unresolved ambiguity explicitly. Return only the improved prompt.";
 let payload=json!({"model":s.model,"stream":false,"temperature":0.2,"messages":[
  {"role":"system","content":instructions},
  {"role":"user","content":q.prompt}
 ]});
 let response=s.http.post(format!("{}/v1/chat/completions",s.llama_url)).json(&payload).send().await.map_err(upstream)?;
 if !response.status().is_success(){let body=response.text().await.unwrap_or_default();return Err((StatusCode::BAD_GATEWAY,body));}
 let result=response.json::<Value>().await.map_err(upstream)?;
 let improved=result["choices"][0]["message"]["content"].as_str()
  .ok_or_else(||(StatusCode::BAD_GATEWAY,"llama.cpp returned no optimized prompt".into()))?.to_string();
 let after=tokenize_text(&s,&improved).await?;
 let budget=q.max_tokens;
 Ok(Json(json!({"original_prompt":q.prompt,"optimized_prompt":improved,
  "original_tokens":before,"optimized_tokens":after,"token_delta":after as i64-before as i64,
  "tokens_saved":before.saturating_sub(after),"budget":budget,
  "within_budget":budget.map(|b|after<=b),"optimizer_model":s.model,
  "method":"llama.cpp_prompt_editor","requires_user_approval":true,
  "note":"Optimization is a proposal. Review it before using it; token reduction does not prove quality improvement."})))
}

async fn conversation(State(s):State<Arc<AppState>>,Path(id):Path<String>)->Result<Json<Value>,(StatusCode,String)> {
 s.store.conversation_messages(&id,200).map(Json).map_err(internal)
}

async fn comfy_status(State(s):State<Arc<AppState>>)->Result<Json<Value>,(StatusCode,String)>{let r=s.http.get(format!("{}/system_stats",s.comfy_url)).send().await.map_err(upstream)?;let status=r.status();let body=r.json::<Value>().await.map_err(upstream)?;if !status.is_success(){return Err((StatusCode::BAD_GATEWAY,body.to_string()));}Ok(Json(body))}
async fn usage(State(s):State<Arc<AppState>>)->Result<Json<Value>,(StatusCode,String)>{s.store.usage_totals().map(Json).map_err(internal)}
#[derive(Deserialize)]struct NewJob{kind:String,#[serde(default)]payload:Value}
async fn create_job(State(s):State<Arc<AppState>>,Json(j):Json<NewJob>)->Result<Json<Value>,(StatusCode,String)>{if j.kind.trim().is_empty(){return Err((StatusCode::BAD_REQUEST,"kind is required".into()));}let id=s.store.create_job(&j.kind,&j.payload.to_string()).map_err(internal)?;Ok(Json(json!({"id":id,"status":"queued","note":"metadata queued; execution adapters are not enabled"})))}
async fn jobs(State(s):State<Arc<AppState>>)->Result<Json<Value>,(StatusCode,String)>{s.store.list_jobs().map(|v|Json(json!({"jobs":v}))).map_err(internal)}
async fn cancel_job(State(s):State<Arc<AppState>>,Path(id):Path<i64>)->Result<Json<Value>,(StatusCode,String)>{let cancelled=s.store.cancel_job(id).map_err(internal)?;Ok(Json(json!({"id":id,"cancelled":cancelled})))}
fn upstream(e:reqwest::Error)->(StatusCode,String){(StatusCode::BAD_GATEWAY,format!("upstream request failed: {e}"))}
fn internal(e:String)->(StatusCode,String){(StatusCode::INTERNAL_SERVER_ERROR,e)}
#[tokio::main]async fn main()->anyhow::Result<()>{let bind=env::var("DREAD_BIND").unwrap_or_else(|_|"127.0.0.1:8765".into());let llama=env::var("DREAD_LLAMA_URL").unwrap_or_else(|_|"http://127.0.0.1:11501".into()).trim_end_matches('/').to_string();let comfy=env::var("DREAD_COMFYUI_URL").unwrap_or_else(|_|"http://127.0.0.1:8188".into()).trim_end_matches('/').to_string();let model=env::var("DREAD_MODEL").unwrap_or_else(|_|"qwen3.8:27b".into());let db=env::var("DREAD_DATABASE").unwrap_or_else(|_|"data/dread.sqlite3".into());let state=Arc::new(AppState{http:reqwest::Client::builder().timeout(Duration::from_secs(300)).build()?,llama_url:llama,comfy_url:comfy,model,store:Store::open(&db)?});let app=Router::new().route("/health",get(health)).route("/api/services",get(services)).route("/api/models",get(models)).route("/api/chat",post(chat)).route("/api/comfyui/status",get(comfy_status)).route("/api/usage",get(usage)).route("/api/tokenize",post(tokenize)).route("/api/prompt/boost",post(boost_prompt)).route("/api/prompt/optimize",post(optimize_prompt)).route("/api/conversations/{id}",get(conversation)).route("/api/jobs",get(jobs).post(create_job)).route("/api/jobs/{id}/cancel",post(cancel_job)).with_state(state);let listener=tokio::net::TcpListener::bind(&bind).await?;println!("PROJECT DREAD UNIVERSAL API listening on http://{bind}; inference=llama.cpp");axum::serve(listener,app).await?;Ok(())}
#[cfg(test)]mod tests{#[test]fn inference_default_is_llama_cpp(){assert_eq!(std::env::var("DREAD_LLAMA_URL").unwrap_or_else(|_|"http://127.0.0.1:11501".into()),"http://127.0.0.1:11501");}}
