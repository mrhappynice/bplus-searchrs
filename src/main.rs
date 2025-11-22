use axum::{
    extract::{Path, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Sse},
    routing::{get, post, put},
    Json, Router,
};
use rust_embed::RustEmbed;
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::CorsLayer;

mod db;
mod llm;
mod search;

// Embeds the "public" folder into the binary
#[derive(RustEmbed)]
#[folder = "public/"]
struct Asset;

// Shared application state
struct AppState {
    db: db::DbManager,
}

#[tokio::main]
async fn main() {
    // Load .env if present
    dotenvy::dotenv().ok();

    // Initialize DB
    let db_manager = db::DbManager::new();
    db_manager.init_schema().expect("Failed to initialize database schema");

    let state = Arc::new(AppState { db: db_manager });

    let app = Router::new()
        // API Routes
        .route("/api/models", get(llm::list_models))
        .route("/api/conversations", get(db::routes::list_conversations).post(db::routes::create_conversation))
        .route("/api/conversations/:id", get(db::routes::get_conversation).delete(db::routes::delete_conversation))
        .route("/api/conversations/:id/notes", put(db::routes::save_note))
        .route("/api/conversations/:id/query", post(handlers::handle_query))
        // DB Persistence Routes
        .route("/api/research/save", post(db::routes::save_db))
        .route("/api/research/load", post(db::routes::load_db))
        .route("/api/research/files", get(db::routes::list_db_files))
        // Autocomplete
        .route("/api/suggest", get(search::suggest))
        // Static Files (Fallback)
        .route("/", get(index_handler))
        .route("/index.html", get(index_handler))
        .fallback(static_handler)
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = 3001;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Server running at http://localhost:{}", port);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// --- Static File Handlers ---

async fn index_handler() -> impl IntoResponse {
    static_handler(Uri::from_static("/index.html")).await
}

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Asset::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
                content.data,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
}

// --- Main Query Handler Module ---
mod handlers {
    use super::*;
    use axum::response::sse::{Event, KeepAlive};
    use futures::stream::Stream;
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct QueryRequest {
        query: String,
        timeframe: Option<String>,
        provider: String,
        model: String,
        #[serde(rename = "systemPrompt")]
        system_prompt: String,
    }

    pub async fn handle_query(
        Path(conversation_id): Path<i64>,
        State(state): State<Arc<super::AppState>>,
        Json(req): Json<QueryRequest>,
    ) -> Sse<impl Stream<Item = Result<Event, axum::BoxError>>> {
        
        // 1. Save User Message
        let _ = state.db.add_message(conversation_id, "user", &req.query, None);

        let stream = async_stream::stream! {
            // 2. Perform Search
            let use_native = std::env::var("USE_NATIVE").unwrap_or_else(|_| "0".to_string()) == "1";
            
            let search_results = if use_native {
                crate::search::native_search(&req.query, req.timeframe.as_deref()).await
            } else {
                crate::search::searxng_search(&req.query, req.timeframe.as_deref()).await
            };

            // Send Results Event
            yield Ok(Event::default().event("results").json_data(&search_results).unwrap());

            if search_results.is_empty() {
                let msg = "No search results found to summarize.";
                yield Ok(Event::default().event("summary-chunk").json_data(serde_json::json!({"text": msg})).unwrap());
                let _ = state.db.add_message(conversation_id, "assistant", msg, Some("[]"));
                return;
            }

            // 3. Prepare LLM Context
            let history = state.db.get_history(conversation_id).unwrap_or_default();
            
            let snippets: String = search_results.iter()
                .map(|r| format!("Title: {}\nURL: {}\nSnippet: {}", r.title, r.url, r.content))
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");
            
            let user_prompt = format!(
                "Based on the following search results, write a clear, concise summary answering my latest prompt: \"{}\".\n\nSearch Results:\n{}", 
                req.query, snippets
            );

            yield Ok(Event::default().event("summary-start").data("{}"));

            // 4. Stream LLM
            let mut full_text = String::new();
            
            // Create the stream based on provider
            let mut llm_stream = crate::llm::stream_completion(
                &req.provider, 
                &req.model, 
                &req.system_prompt, 
                history, 
                &user_prompt
            ).await;

            while let Some(chunk_res) = futures::StreamExt::next(&mut llm_stream).await {
                match chunk_res {
                    Ok(text) => {
                        full_text.push_str(&text);
                        yield Ok(Event::default().event("summary-chunk").json_data(serde_json::json!({"text": text})).unwrap());
                    },
                    Err(e) => {
                        yield Ok(Event::default().event("error").json_data(serde_json::json!({"message": e.to_string()})).unwrap());
                    }
                }
            }

            // 5. Save Assistant Message
            let sources_json = serde_json::to_string(&search_results).unwrap_or_default();
            let msg_id = state.db.add_message(conversation_id, "assistant", &full_text, Some(&sources_json)).unwrap_or(0);
            
            yield Ok(Event::default().event("summary-done").json_data(serde_json::json!({"messageId": msg_id})).unwrap());
        };

        Sse::new(stream).keep_alive(KeepAlive::default())
    }
}