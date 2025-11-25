use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use anyhow::Result;

pub struct DbManager {
    pub conn: Arc<Mutex<Connection>>,
    current_file: Arc<Mutex<Option<PathBuf>>>,
}

impl DbManager {
    pub fn new() -> Self {
        let conn = Connection::open_in_memory().expect("Failed to open memory DB");
        Self {
            conn: Arc::new(Mutex::new(conn)),
            current_file: Arc::new(Mutex::new(None)),
        }
    }

    fn get_storage_dir() -> PathBuf {
        std::env::current_exe()
            .map(|p| p.parent().unwrap().to_path_buf())
            .unwrap_or_else(|_| std::env::current_dir().unwrap())
    }

    pub fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
            
            CREATE TABLE IF NOT EXISTS conversations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                sources TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );
            
            CREATE TABLE IF NOT EXISTS notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER NOT NULL UNIQUE,
                content TEXT NOT NULL,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS search_providers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                type TEXT NOT NULL,
                api_url TEXT,
                api_headers TEXT,
                result_path TEXT,
                title_path TEXT, 
                url_path TEXT,
                content_path TEXT,
                is_enabled BOOLEAN DEFAULT 1
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                content, content='messages', content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS messages_after_insert AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
            END;
            "
        )?;

        // Ensure defaults exist (DDG, Qwant, etc)
        // We use INSERT OR IGNORE logic via checking name presence
        let defaults = vec![
            ("DuckDuckGo", "native", "native_ddg"),
            ("Qwant", "native", "native_qwant"), // Ensure Qwant is here
            ("Mojeek", "native", "native_mojeek"),
            ("Wikipedia", "native", "native_wiki"),
            ("Reddit", "native", "native_reddit"),
            ("StackExchange", "native", "native_stack"),
        ];

        if std::env::var("SEARXNG_URL").is_ok() {
             // Basic check if it exists
             let count: i64 = conn.query_row("SELECT count(*) FROM search_providers WHERE api_url = 'native_searxng'", [], |r| r.get(0)).unwrap_or(0);
             if count == 0 {
                 conn.execute("INSERT INTO search_providers (name, type, api_url) VALUES (?, ?, ?)", 
                   params!["SearXNG", "native", "native_searxng"]).unwrap();
             }
        }

        for (name, ptype, url) in defaults {
            let count: i64 = conn.query_row("SELECT count(*) FROM search_providers WHERE api_url = ?", params![url], |r| r.get(0)).unwrap_or(0);
            if count == 0 {
                conn.execute(
                    "INSERT INTO search_providers (name, type, api_url) VALUES (?, ?, ?)",
                    params![name, ptype, url],
                ).unwrap();
            }
        }

        Ok(())
    }

    pub fn add_message(&self, conv_id: i64, role: &str, content: &str, sources: Option<&str>) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO messages (conversation_id, role, content, sources) VALUES (?, ?, ?, ?)",
            params![conv_id, role, content, sources],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_history(&self, conv_id: i64) -> Result<Vec<crate::llm::Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT role, content FROM messages WHERE conversation_id = ? ORDER BY created_at ASC")?;
        let rows = stmt.query_map(params![conv_id], |row| {
            Ok(crate::llm::Message { role: row.get(0)?, content: row.get(1)? })
        })?;
        let mut history = Vec::new();
        for r in rows { history.push(r?); }
        if let Some(last) = history.last() {
            if last.role == "user" { history.pop(); }
        }
        Ok(history)
    }

    pub fn get_providers(&self, ids: Option<Vec<i64>>) -> Result<Vec<crate::search::ProviderConfig>> {
        let conn = self.conn.lock().unwrap();
        let query = "SELECT id, name, type, api_url, api_headers, result_path, title_path, url_path, content_path FROM search_providers WHERE is_enabled = 1".to_string();
        let mut stmt = conn.prepare(&query)?;
        
        let iter = stmt.query_map([], |row| {
            Ok(crate::search::ProviderConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                type_: row.get(2)?,
                api_url: row.get(3)?,
                api_headers: row.get(4)?,
                result_path: row.get(5)?,
                title_path: row.get(6)?,
                url_path: row.get(7)?,
                content_path: row.get(8)?,
            })
        })?;

        let mut providers = Vec::new();
        for p in iter { 
            let p = p?;
            if let Some(req_ids) = &ids {
                if req_ids.contains(&p.id) { providers.push(p); }
            } else {
                providers.push(p);
            }
        }
        Ok(providers)
    }

    pub fn load_file(&self, filename: &str) -> Result<()> {
        let path = Self::get_storage_dir().join(filename);
        let new_conn = Connection::open(&path)?;
        {
            let mut conn_guard = self.conn.lock().unwrap();
            *conn_guard = new_conn;
            let mut path_guard = self.current_file.lock().unwrap();
            *path_guard = Some(path);
        }
        self.init_schema()?;
        Ok(())
    }

    pub fn save_to_file(&self, filename: &str) -> Result<()> {
        let path = Self::get_storage_dir().join(filename);
        let conn = self.conn.lock().unwrap();
        conn.backup(rusqlite::DatabaseName::Main, &path, None)?;
        Ok(())
    }
}

pub mod routes {
    use super::*;
    use axum::{Json, extract::{Path, State}, http::StatusCode};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize)]
    pub struct Conversation { id: i64, title: String, created_at: String }
    pub async fn list_conversations(State(state): State<Arc<crate::AppState>>) -> Json<Vec<Conversation>> {
        let conn = state.db.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, title, created_at FROM conversations ORDER BY created_at DESC").unwrap();
        let rows = stmt.query_map([], |r| Ok(Conversation{id:r.get(0)?, title:r.get(1)?, created_at:r.get(2)?})).unwrap();
        Json(rows.map(|r| r.unwrap()).collect())
    }
    
    #[derive(Deserialize)] 
    pub struct CreateConv { title: Option<String> }
    
    pub async fn create_conversation(State(state): State<Arc<crate::AppState>>, Json(req): Json<CreateConv>) -> Json<serde_json::Value> {
        let conn = state.db.conn.lock().unwrap();
        conn.execute("INSERT INTO conversations (title) VALUES (?)", params![req.title.unwrap_or("New Chat".into())]).unwrap();
        Json(serde_json::json!({ "id": conn.last_insert_rowid() }))
    }

    pub async fn get_conversation(Path(id): Path<i64>, State(state): State<Arc<crate::AppState>>) -> Json<serde_json::Value> {
        let conn = state.db.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT role, content, sources FROM messages WHERE conversation_id = ? ORDER BY created_at ASC").unwrap();
        let msgs: Vec<serde_json::Value> = stmt.query_map(params![id], |r| {
            Ok(serde_json::json!({ "role": r.get::<_,String>(0)?, "content": r.get::<_,String>(1)?, "sources": r.get::<_,Option<String>>(2)? }))
        }).unwrap().map(|r| r.unwrap()).collect();
        let note: Option<String> = conn.query_row("SELECT content FROM notes WHERE conversation_id = ?", params![id], |r| r.get(0)).ok();
        Json(serde_json::json!({ "messages": msgs, "note_content": note }))
    }

    pub async fn delete_conversation(Path(id): Path<i64>, State(state): State<Arc<crate::AppState>>) -> StatusCode {
        state.db.conn.lock().unwrap().execute("DELETE FROM conversations WHERE id = ?", params![id]).unwrap();
        StatusCode::NO_CONTENT
    }

    #[derive(Deserialize)] 
    pub struct NoteReq { content: String }
    pub async fn save_note(Path(id): Path<i64>, State(state): State<Arc<crate::AppState>>, Json(req): Json<NoteReq>) -> Json<serde_json::Value> {
        state.db.conn.lock().unwrap().execute("INSERT INTO notes (conversation_id, content) VALUES (?, ?) ON CONFLICT(conversation_id) DO UPDATE SET content=excluded.content", params![id, req.content]).unwrap();
        Json(serde_json::json!({"status": "ok"}))
    }

    // --- Provider Routes ---

    pub async fn list_providers(State(state): State<Arc<crate::AppState>>) -> Json<Vec<crate::search::ProviderConfig>> {
        let providers = state.db.get_providers(None).unwrap_or_default();
        Json(providers)
    }

    #[derive(Deserialize)]
    pub struct AddProviderReq {
        name: String,
        api_url: String,
        api_headers: String,
        result_path: String,
        title_path: String,
        url_path: String,
        content_path: String
    }

    pub async fn add_provider(State(state): State<Arc<crate::AppState>>, Json(req): Json<AddProviderReq>) -> Json<serde_json::Value> {
        let conn = state.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO search_providers (name, type, api_url, api_headers, result_path, title_path, url_path, content_path) 
             VALUES (?, 'generic', ?, ?, ?, ?, ?, ?)",
            params![req.name, req.api_url, req.api_headers, req.result_path, req.title_path, req.url_path, req.content_path]
        ).unwrap();
        Json(serde_json::json!({ "id": conn.last_insert_rowid() }))
    }

    pub async fn delete_provider(Path(id): Path<i64>, State(state): State<Arc<crate::AppState>>) -> StatusCode {
        let conn = state.db.conn.lock().unwrap();
        conn.execute("DELETE FROM search_providers WHERE id = ?", params![id]).unwrap();
        StatusCode::NO_CONTENT
    }

    #[derive(Deserialize)] 
    pub struct FileReq { filename: String }
    pub async fn save_db(State(state): State<Arc<crate::AppState>>, Json(req): Json<FileReq>) -> Json<serde_json::Value> {
        let mut f = req.filename; if !f.ends_with(".db") { f.push_str(".db"); }
        state.db.save_to_file(&f).unwrap();
        Json(serde_json::json!({"message": "saved"}))
    }
    pub async fn load_db(State(state): State<Arc<crate::AppState>>, Json(req): Json<FileReq>) -> Json<serde_json::Value> {
        state.db.load_file(&req.filename).unwrap();
        Json(serde_json::json!({"message": "loaded"}))
    }
    pub async fn list_db_files() -> Json<Vec<String>> {
        let dir = DbManager::get_storage_dir();
        let files = std::fs::read_dir(dir).unwrap().flatten()
            .filter(|e| e.path().extension().map_or(false, |x| x=="db"))
            .map(|e| e.file_name().to_string_lossy().to_string()).collect();
        Json(files)
    }
}