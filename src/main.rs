use axum::{
    extract::{Path, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Sse},
    routing::{get, post, put, delete},
    Json, Router,
};
use rust_embed::RustEmbed;
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::CorsLayer;

mod db;
mod llm;
mod search;

#[derive(RustEmbed)]
#[folder = "public/"]
struct Asset;

struct AppState {
    db: db::DbManager,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let db_manager = db::DbManager::new();
    db_manager.init_schema().expect("Failed to init DB");
    let state = Arc::new(AppState { db: db_manager });

    let app = Router::new()
        .route("/api/models", get(llm::list_models))
        .route("/api/suggest", get(search::suggest))
        .route("/api/conversations", get(db::routes::list_conversations).post(db::routes::create_conversation))
        .route("/api/conversations/:id", get(db::routes::get_conversation).delete(db::routes::delete_conversation))
        .route("/api/conversations/:id/notes", put(db::routes::save_note))
        .route("/api/conversations/:id/query", post(handlers::handle_query))
        .route("/api/providers", get(db::routes::list_providers).post(db::routes::add_provider))
        .route("/api/providers/:id", delete(db::routes::delete_provider))
        .route("/api/research/save", post(db::routes::save_db))
        .route("/api/research/load", post(db::routes::load_db))
        .route("/api/research/files", get(db::routes::list_db_files))
        .route("/", get(index_handler))
        .route("/index.html", get(index_handler))
        .fallback(static_handler)
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = 3001;
    println!("Server running at http://localhost:{}", port);
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn index_handler() -> impl IntoResponse { static_handler(Uri::from_static("/index.html")).await }

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Asset::get(path) {
        Some(content) => ([(axum::http::header::CONTENT_TYPE, mime_guess::from_path(path).first_or_octet_stream().as_ref())], content.data).into_response(),
        None => (StatusCode::NOT_FOUND, "404").into_response(),
    }
}

mod handlers {
    use super::*;
    use axum::response::sse::{Event, KeepAlive};
    use futures::stream::Stream;
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct QueryRequest {
        query: String,
        timeframe: Option<String>,
        providers: Option<Vec<i64>>,
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
        
        let _ = state.db.add_message(conversation_id, "user", &req.query, None);

        let stream = async_stream::stream! {
            let providers_config = state.db.get_providers(req.providers).unwrap_or_default();
            
            // REVERTED USER AGENT to "bplus-native/1.0" for better native results
            let client = reqwest::Client::builder().user_agent("bplus-native/1.0").timeout(std::time::Duration::from_secs(15)).build().unwrap();
            
            let mut search_results = crate::search::perform_search(
                client, 
                providers_config, 
                req.query.clone(),
                req.timeframe.clone()
            ).await;

            if search_results.len() > 15 { search_results.truncate(15); }

            yield Ok(Event::default().event("results").json_data(&search_results).unwrap());

            if search_results.is_empty() {
                yield Ok(Event::default().event("summary-chunk").json_data(serde_json::json!({"text": "No search results found to summarize."})).unwrap());
                let _ = state.db.add_message(conversation_id, "assistant", "No search results found to summarize.", Some("[]"));
                return;
            }

            let history = state.db.get_history(conversation_id).unwrap_or_default();
            
            let snippets: String = search_results.iter()
                .map(|r| format!("[{}] {}\nURL: {}\nSnippet: {}", r.engine, r.title, r.url, r.content))
                .collect::<Vec<_>>().join("\n\n---\n\n");
            
            // ADDED CURRENT DATE
            let current_date = chrono::Local::now().format("%Y-%m-%d").to_string();
            let user_prompt = format!(
                "Current Date: {}\nQuery: \"{}\"\n\nBased on the following search results, write a clear, concise summary answering the query. If results mention this date, they are current.\n\nSearch Results:\n{}", 
                current_date, req.query, snippets
            );

            yield Ok(Event::default().event("summary-start").data("{}"));

            let mut full_text = String::new();
            let mut llm_stream = crate::llm::stream_completion(&req.provider, &req.model, &req.system_prompt, history, &user_prompt).await;

            while let Some(chunk) = futures::StreamExt::next(&mut llm_stream).await {
                match chunk {
                    Ok(text) => {
                        full_text.push_str(&text);
                        yield Ok(Event::default().event("summary-chunk").json_data(serde_json::json!({"text": text})).unwrap());
                    },
                    Err(e) => {
                        yield Ok(Event::default().event("error").json_data(serde_json::json!({"message": e.to_string()})).unwrap());
                    }
                }
            }

            let sources_json = serde_json::to_string(&search_results).unwrap_or_default();
            let msg_id = state.db.add_message(conversation_id, "assistant", &full_text, Some(&sources_json)).unwrap_or(0);
            yield Ok(Event::default().event("summary-done").json_data(serde_json::json!({"messageId": msg_id})).unwrap());
        };

        Sse::new(stream).keep_alive(KeepAlive::default())
    }
}