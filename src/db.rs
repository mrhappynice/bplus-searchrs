use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use anyhow::Result;

pub struct DbManager {
    // We use a Mutex<Connection> because we might swap the underlying file 
    // (Load DB) or backup the connection.
    conn: Arc<Mutex<Connection>>,
    current_file: Arc<Mutex<Option<PathBuf>>>, // None = :memory:
}

impl DbManager {
    pub fn new() -> Self {
        // Default to memory to match server.js logic
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

            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                content, content='messages', content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS messages_after_insert AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS messages_after_delete AFTER DELETE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS messages_after_update AFTER UPDATE ON messages BEGIN
                UPDATE messages_fts SET content = new.content WHERE rowid = old.id;
            END;
            "
        )?;
        Ok(())
    }

    // --- Core Operations ---

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
        // Exclude the very last message (which is usually the user query currently being processed, 
        // but in the Node logic, it filters specifically. We will just fetch previous messages).
        // Node logic: SELECT role, content FROM messages ... AND id NOT IN (SELECT id ... ORDER BY created_at DESC LIMIT 1)
        let mut stmt = conn.prepare(
            "SELECT role, content FROM messages WHERE conversation_id = ? ORDER BY created_at ASC"
        )?;
        
        let rows = stmt.query_map(params![conv_id], |row| {
            Ok(crate::llm::Message {
                role: row.get(0)?,
                content: row.get(1)?,
            })
        })?;

        // Convert to vec and exclude last user message if needed, 
        // but simplified here: LLMs usually handle the duplicate user prompt fine if it's in history + prompt.
        // We will return all history except the current turn to avoid duplication if the caller appends the prompt manually.
        let mut history = Vec::new();
        for r in rows { history.push(r?); }
        
        // Remove the last one if it matches the current query context (optional optimization)
        if let Some(last) = history.last() {
            if last.role == "user" {
                history.pop(); 
            }
        }
        Ok(history)
    }

    pub fn load_file(&self, filename: &str) -> Result<()> {
        let path = Self::get_storage_dir().join(filename);
        let new_conn = Connection::open(&path)?;
        
        // Swap connections
        {
            let mut conn_guard = self.conn.lock().unwrap();
            *conn_guard = new_conn;
            let mut path_guard = self.current_file.lock().unwrap();
            *path_guard = Some(path);
        }
        // Re-init schema/pragmas
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

// --- Routes for DB (Controllers) ---
pub mod routes {
    use super::*;
    use axum::{Json, extract::{Path, State}, http::StatusCode};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize)]
    pub struct Conversation {
        id: i64,
        title: String,
        created_at: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        messages: Option<Vec<MessageRow>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        note_content: Option<String>,
    }

    #[derive(Serialize)]
    pub struct MessageRow {
        id: i64,
        role: String,
        content: String,
        sources: Option<String>,
        created_at: String,
    }

    pub async fn list_conversations(State(state): State<Arc<crate::AppState>>) -> Json<Vec<Conversation>> {
        let conn = state.db.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, title, created_at FROM conversations ORDER BY created_at DESC").unwrap();
        let rows = stmt.query_map([], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                messages: None,
                note_content: None,
            })
        }).unwrap();
        let mut res = Vec::new();
        for r in rows { res.push(r.unwrap()); }
        Json(res)
    }

    #[derive(Deserialize)]
    pub struct CreateConvReq { title: Option<String> }
    
    pub async fn create_conversation(State(state): State<Arc<crate::AppState>>, Json(req): Json<CreateConvReq>) -> Json<serde_json::Value> {
        let conn = state.db.conn.lock().unwrap();
        let title = req.title.unwrap_or_else(|| "New Conversation".to_string());
        conn.execute("INSERT INTO conversations (title) VALUES (?)", params![title]).unwrap();
        let id = conn.last_insert_rowid();
        Json(serde_json::json!({ "id": id, "title": title }))
    }

    pub async fn get_conversation(Path(id): Path<i64>, State(state): State<Arc<crate::AppState>>) -> Result<Json<Conversation>, StatusCode> {
        let conn = state.db.conn.lock().unwrap();
        
        let mut conv_stmt = conn.prepare(
            "SELECT c.id, c.title, c.created_at, n.content as note_content 
             FROM conversations c
             LEFT JOIN notes n ON c.id = n.conversation_id
             WHERE c.id = ?"
        ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let mut conv = conv_stmt.query_row(params![id], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                messages: Some(Vec::new()),
                note_content: row.get(3).unwrap_or(None),
            })
        }).map_err(|_| StatusCode::NOT_FOUND)?;

        let mut msg_stmt = conn.prepare("SELECT id, role, content, sources, created_at FROM messages WHERE conversation_id = ? ORDER BY created_at ASC").unwrap();
        let msgs = msg_stmt.query_map(params![id], |row| {
            Ok(MessageRow {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                sources: row.get(3)?,
                created_at: row.get(4)?,
            })
        }).unwrap();

        let mut messages = Vec::new();
        for m in msgs { messages.push(m.unwrap()); }
        conv.messages = Some(messages);

        Ok(Json(conv))
    }

    pub async fn delete_conversation(Path(id): Path<i64>, State(state): State<Arc<crate::AppState>>) -> StatusCode {
        let conn = state.db.conn.lock().unwrap();
        match conn.execute("DELETE FROM conversations WHERE id = ?", params![id]) {
            Ok(changes) if changes > 0 => StatusCode::NO_CONTENT,
            _ => StatusCode::NOT_FOUND
        }
    }

    #[derive(Deserialize)]
    pub struct NoteReq { content: String }
    pub async fn save_note(Path(id): Path<i64>, State(state): State<Arc<crate::AppState>>, Json(req): Json<NoteReq>) -> Json<serde_json::Value> {
        let conn = state.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO notes (conversation_id, content, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(conversation_id) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at",
            params![id, req.content]
        ).unwrap();
        Json(serde_json::json!({ "message": "Notes saved" }))
    }

    #[derive(Deserialize)]
    pub struct FileReq { filename: String }
    
    pub async fn save_db(State(state): State<Arc<crate::AppState>>, Json(req): Json<FileReq>) -> Json<serde_json::Value> {
        let mut fname = req.filename;
        if !fname.ends_with(".db") { fname.push_str(".db"); }
        match state.db.save_to_file(&fname) {
            Ok(_) => Json(serde_json::json!({ "message": format!("Saved to {}", fname) })),
            Err(e) => Json(serde_json::json!({ "error": e.to_string() }))
        }
    }

    pub async fn load_db(State(state): State<Arc<crate::AppState>>, Json(req): Json<FileReq>) -> Json<serde_json::Value> {
        match state.db.load_file(&req.filename) {
            Ok(_) => Json(serde_json::json!({ "message": format!("Loaded {}", req.filename) })),
            Err(e) => Json(serde_json::json!({ "error": e.to_string() }))
        }
    }

    pub async fn list_db_files() -> Json<Vec<String>> {
        let dir = DbManager::get_storage_dir();
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|s| s == "db").unwrap_or(false) {
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        files.push(name.to_string());
                    }
                }
            }
        }
        Json(files)
    }
}