use rusqlite::{Connection, params};
use serde::Serialize;
use std::{path::Path, sync::{Arc, Mutex}};

#[derive(Clone)]
pub struct Store(Arc<Mutex<Connection>>);
#[derive(Serialize)]
pub struct Job { pub id:i64, pub kind:String, pub status:String, pub payload:String, pub created_at:String, pub updated_at:String }

impl Store {
 pub fn open(path:&str)->Result<Self,rusqlite::Error>{
  if let Some(parent)=Path::new(path).parent(){std::fs::create_dir_all(parent).map_err(|e|rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;}
  let conn=Connection::open(path)?;
  conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
   CREATE TABLE IF NOT EXISTS conversations(id TEXT PRIMARY KEY,title TEXT NOT NULL DEFAULT '',created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
   CREATE TABLE IF NOT EXISTS messages(id INTEGER PRIMARY KEY AUTOINCREMENT,conversation_id TEXT NOT NULL,role TEXT NOT NULL,content TEXT NOT NULL,model TEXT NOT NULL DEFAULT '',created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,FOREIGN KEY(conversation_id) REFERENCES conversations(id));
   CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id,id);
   CREATE TABLE IF NOT EXISTS token_usage(id INTEGER PRIMARY KEY AUTOINCREMENT,conversation_id TEXT NOT NULL,model TEXT NOT NULL,prompt_tokens INTEGER NOT NULL,completion_tokens INTEGER NOT NULL,total_tokens INTEGER NOT NULL,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
   CREATE TABLE IF NOT EXISTS jobs(id INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'queued',payload TEXT NOT NULL,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
   CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status,id);")?;
  Ok(Self(Arc::new(Mutex::new(conn))))
 }
 pub fn save_exchange(&self,c:&str,m:&str,u:&str,a:&str,t:(i64,i64,i64))->Result<(),String>{
  let conn=self.0.lock().map_err(|e|e.to_string())?;let tx=conn.unchecked_transaction().map_err(|e|e.to_string())?;
  tx.execute("INSERT OR IGNORE INTO conversations(id) VALUES(?1)",[c]).map_err(|e|e.to_string())?;
  tx.execute("INSERT INTO messages(conversation_id,role,content,model) VALUES(?1,'user',?2,?3)",params![c,u,m]).map_err(|e|e.to_string())?;
  tx.execute("INSERT INTO messages(conversation_id,role,content,model) VALUES(?1,'assistant',?2,?3)",params![c,a,m]).map_err(|e|e.to_string())?;
  tx.execute("INSERT INTO token_usage(conversation_id,model,prompt_tokens,completion_tokens,total_tokens) VALUES(?1,?2,?3,?4,?5)",params![c,m,t.0,t.1,t.2]).map_err(|e|e.to_string())?;
  tx.commit().map_err(|e|e.to_string())
 }
 pub fn conversation_messages(&self,id:&str,limit:i64)->Result<serde_json::Value,String>{
  let c=self.0.lock().map_err(|e|e.to_string())?;
  let mut s=c.prepare("SELECT role,content,model,created_at FROM messages WHERE conversation_id=?1 ORDER BY id DESC LIMIT ?2").map_err(|e|e.to_string())?;
  let rows=s.query_map(params![id,limit.clamp(1,500)],|r|Ok(serde_json::json!({"role":r.get::<_,String>(0)?,"content":r.get::<_,String>(1)?,"model":r.get::<_,String>(2)?,"created_at":r.get::<_,String>(3)?}))).map_err(|e|e.to_string())?;
  let mut v=rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;v.reverse();Ok(serde_json::json!({"conversation_id":id,"messages":v}))
 }
 pub fn usage_totals(&self)->Result<serde_json::Value,String>{
  let c=self.0.lock().map_err(|e|e.to_string())?;
  let r:(i64,i64,i64,i64)=c.query_row("SELECT COUNT(*),COALESCE(SUM(prompt_tokens),0),COALESCE(SUM(completion_tokens),0),COALESCE(SUM(total_tokens),0) FROM token_usage",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
  Ok(serde_json::json!({"requests":r.0,"prompt_tokens":r.1,"completion_tokens":r.2,"total_tokens":r.3}))
 }
 pub fn create_job(&self,k:&str,p:&str)->Result<i64,String>{let c=self.0.lock().map_err(|e|e.to_string())?;c.execute("INSERT INTO jobs(kind,payload) VALUES(?1,?2)",params![k,p]).map_err(|e|e.to_string())?;Ok(c.last_insert_rowid())}
 pub fn list_jobs(&self)->Result<Vec<Job>,String>{let c=self.0.lock().map_err(|e|e.to_string())?;let mut s=c.prepare("SELECT id,kind,status,payload,created_at,updated_at FROM jobs ORDER BY id DESC LIMIT 100").map_err(|e|e.to_string())?;let rows=s.query_map([],|r|Ok(Job{id:r.get(0)?,kind:r.get(1)?,status:r.get(2)?,payload:r.get(3)?,created_at:r.get(4)?,updated_at:r.get(5)?})).map_err(|e|e.to_string())?;rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())}
 pub fn cancel_job(&self,id:i64)->Result<bool,String>{let c=self.0.lock().map_err(|e|e.to_string())?;let n=c.execute("UPDATE jobs SET status='cancelled',updated_at=CURRENT_TIMESTAMP WHERE id=?1 AND status='queued'",[id]).map_err(|e|e.to_string())?;Ok(n>0)}
}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn persists_exchange_usage_and_conversation_history() {
  let path=std::env::temp_dir().join(format!("dread-store-{}.sqlite",uuid::Uuid::new_v4()));
  let store=Store::open(path.to_str().unwrap()).unwrap();
  store.save_exchange("c-1","test-model","hello","world",(3,2,5)).unwrap();
  let history=store.conversation_messages("c-1",20).unwrap();
  assert_eq!(history["messages"].as_array().unwrap().len(),2);
  assert_eq!(history["messages"][0]["role"],"user");
  assert_eq!(history["messages"][1]["content"],"world");
  let usage=store.usage_totals().unwrap();
  assert_eq!(usage["requests"],1);
  assert_eq!(usage["total_tokens"],5);
  drop(store);
  let _=std::fs::remove_file(&path);
  let _=std::fs::remove_file(path.with_extension("sqlite-wal"));
  let _=std::fs::remove_file(path.with_extension("sqlite-shm"));
 }
 #[test]
 fn only_queued_jobs_can_be_cancelled() {
  let path=std::env::temp_dir().join(format!("dread-jobs-{}.sqlite",uuid::Uuid::new_v4()));
  let store=Store::open(path.to_str().unwrap()).unwrap();
  let id=store.create_job("test","{}").unwrap();
  assert!(store.cancel_job(id).unwrap());
  assert!(!store.cancel_job(id).unwrap());
  assert_eq!(store.list_jobs().unwrap()[0].status,"cancelled");
  drop(store);
  let _=std::fs::remove_file(&path);
  let _=std::fs::remove_file(path.with_extension("sqlite-wal"));
  let _=std::fs::remove_file(path.with_extension("sqlite-shm"));
 }
}

#[cfg(test)]
mod persistence_tests {
 use super::*;
 fn test_store()->(Store,String){
  let path=std::env::temp_dir().join(format!("dread-store-{}.sqlite3",uuid::Uuid::new_v4()));
  let s=Store::open(path.to_str().unwrap()).unwrap();
  (s,path.to_string_lossy().into_owned())
 }
 #[test]
 fn exchange_is_persisted_and_retrievable(){
  let (s,path)=test_store();
  s.save_exchange("c1","test-model","hello","world",(3,2,5)).unwrap();
  let history=s.conversation_messages("c1",20).unwrap();
  assert_eq!(history["messages"].as_array().unwrap().len(),2);
  assert_eq!(history["messages"][0]["role"],"user");
  assert_eq!(history["messages"][1]["content"],"world");
  let usage=s.usage_totals().unwrap();
  assert_eq!(usage["total_tokens"],5);
  drop(s);let _=std::fs::remove_file(path);
 }
 #[test]
 fn queued_job_can_be_cancelled_only_once(){
  let (s,path)=test_store();
  let id=s.create_job("review_only","{}").unwrap();
  assert!(s.cancel_job(id).unwrap());
  assert!(!s.cancel_job(id).unwrap());
  let jobs=s.list_jobs().unwrap();
  assert_eq!(jobs[0].status,"cancelled");
  drop(s);let _=std::fs::remove_file(path);
 }
}
