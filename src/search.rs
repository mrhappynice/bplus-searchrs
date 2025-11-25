use serde::{Deserialize, Serialize};
use scraper::{Html, Selector};
use axum::{Json, extract::Query};
use futures::future::join_all;
use reqwest::Client;
use std::pin::Pin;
use std::future::Future;
use std::collections::HashSet;
use std::path::PathBuf;
use rusqlite::{Connection, OpenFlags, params};

#[derive(Serialize, Clone, Debug)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub engine: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProviderConfig {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub api_url: Option<String>,
    pub api_headers: Option<String>,
    pub result_path: Option<String>,
    pub title_path: Option<String>,
    pub url_path: Option<String>,
    pub content_path: Option<String>,
    pub is_enabled: bool, 
}

pub trait SearchProvider: Send + Sync {
    fn search(&self, client: Client, query: String, timeframe: Option<String>) -> Pin<Box<dyn Future<Output = Vec<SearchResult>> + Send>>;
}

// 1. Generic API Provider
struct GenericApiProvider {
    config: ProviderConfig,
}

impl GenericApiProvider {
    fn extract(&self, val: &serde_json::Value, path: Option<&String>) -> String {
        let path = match path { Some(p) if !p.is_empty() => p, _ => return "".to_string() };
        let parts: Vec<&str> = path.split('.').collect();
        let mut curr = val;
        
        for part in parts {
            if let Some(idx) = part.parse::<usize>().ok() {
                if let Some(arr) = curr.as_array() { 
                    if idx < arr.len() { curr = &arr[idx]; } else { return "".to_string(); }
                } else { return "".to_string(); }
            } else {
                curr = &curr[part];
            }
        }

        match curr {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => "".to_string()
        }
    }
}

impl GenericApiProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self { config }
    }
}

impl SearchProvider for GenericApiProvider {
    fn search(&self, client: Client, query: String, _timeframe: Option<String>) -> Pin<Box<dyn Future<Output = Vec<SearchResult>> + Send>> {
        let config = self.config.clone();
        Box::pin(async move {
            let url_tmpl = config.api_url.as_deref().unwrap_or("");
            if url_tmpl.is_empty() { return vec![]; }
            let url = url_tmpl.replace("{q}", &urlencoding::encode(&query));
            
            let mut req = client.get(&url);
            req = req.header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");

            if let Some(h_str) = &config.api_headers {
                if let Ok(headers) = serde_json::from_str::<std::collections::HashMap<String, String>>(h_str) {
                    for (k, v) in headers { req = req.header(&k, &v); }
                }
            }

            let mut results = Vec::new();
            match req.send().await {
                Ok(resp) => {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        let mut root = &json;
                        if let Some(rpath) = &config.result_path {
                            for part in rpath.split('.') {
                                if !part.is_empty() { root = &root[part]; }
                            }
                        }
                        
                        if let Some(arr) = root.as_array() {
                            for item in arr {
                                let title = GenericApiProvider::new(config.clone()).extract(item, config.title_path.as_ref());
                                let url = GenericApiProvider::new(config.clone()).extract(item, config.url_path.as_ref());
                                
                                if !url.is_empty() {
                                    results.push(SearchResult {
                                        title: if title.is_empty() { "No Title".into() } else { title },
                                        url,
                                        content: GenericApiProvider::new(config.clone()).extract(item, config.content_path.as_ref()),
                                        engine: config.name.clone()
                                    });
                                }
                            }
                        }
                    }
                },
                Err(e) => println!("Error: Request failed: {}", e),
            }
            results
        })
    }
}

// 2. Native Provider Wrapper
struct NativeProvider {
    id: String, 
    _name: String,
}

impl SearchProvider for NativeProvider {
    fn search(&self, client: Client, query: String, timeframe: Option<String>) -> Pin<Box<dyn Future<Output = Vec<SearchResult>> + Send>> {
        let id = self.id.clone();
        Box::pin(async move {
            match id.as_str() {
                "native_local_db" => local_db_search(query).await,
                "native_ddg" => ddg_web(client, query, timeframe).await,
                "native_qwant" => qwant_web(client, query).await,
                "native_mojeek" => mojeek_web(client, query).await,
                "native_wiki" => wikipedia_web(client, query).await,
                "native_reddit" => reddit_web(client, query).await,
                "native_stack" => stackexchange_web(client, query).await,
                "native_searxng" => searxng_search(client, query, timeframe).await,
                _ => vec![]
            }
        })
    }
}

pub async fn perform_search(
    client: Client, 
    providers: Vec<ProviderConfig>, 
    query: String,
    timeframe: Option<String>
) -> Vec<SearchResult> {
    let mut futures = Vec::new();
    
    // Default to Local Database if no providers selected
    let effective_providers = if providers.is_empty() {
        vec![
            ProviderConfig { 
                id: 0, 
                name: "Local Database".into(), 
                type_: "native".into(), 
                api_url: Some("native_local_db".into()), 
                api_headers: None, result_path: None, title_path: None, url_path: None, content_path: None,
                is_enabled: true 
            },
        ]
    } else {
        providers
    };

    for p in effective_providers {
        let provider: Box<dyn SearchProvider> = if p.type_ == "generic" {
            Box::new(GenericApiProvider { config: p })
        } else {
            Box::new(NativeProvider { 
                id: p.api_url.clone().unwrap_or_default(), 
                _name: p.name.clone() 
            })
        };
        futures.push(provider.search(client.clone(), query.clone(), timeframe.clone()));
    }

    let results_list = join_all(futures).await;
    let mut all = Vec::new();
    for res in results_list { all.extend(res); }

    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for r in all {
        if !seen.contains(&r.url) {
            seen.insert(r.url.clone());
            unique.push(r);
        }
    }
    
    // Sort relevance locally
    let q_low = query.to_lowercase();
    unique.sort_by(|a, b| {
        let ascore = if a.title.to_lowercase().contains(&q_low) { 1 } else { 0 };
        let bscore = if b.title.to_lowercase().contains(&q_low) { 1 } else { 0 };
        bscore.cmp(&ascore)
    });
    
    unique
}

// --- Native Impls ---

async fn local_db_search(query: String) -> Vec<SearchResult> {
    let files: Vec<PathBuf> = match std::fs::read_dir(".") {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |ext| ext == "db"))
            .collect(),
        Err(_) => return vec![]
    };

    if files.is_empty() { return vec![]; }

    let task = tokio::task::spawn_blocking(move || {
        let mut results = Vec::new();
        // High limit to ensure we find hits across different conversations
        let limit_raw_hits = 100; 

        for path in files {
            let filename = path.file_name().unwrap().to_string_lossy().to_string();
            
            if let Ok(conn) = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
                
                // 1. Search Notes (Summaries)
                // Notes are high-value dense information, search them first
                let notes_sql = "
                    SELECT n.content, c.title, n.updated_at
                    FROM notes n 
                    JOIN conversations c ON n.conversation_id = c.id
                    WHERE n.content LIKE '%' || ? || '%' 
                       OR c.title LIKE '%' || ? || '%'
                    LIMIT 3
                ";
                
                if let Ok(mut notes_stmt) = conn.prepare(notes_sql) {
                    let notes_rows = notes_stmt.query_map(params![query, query], |row| {
                        Ok(SearchResult {
                            title: format!("[Local: {}] NOTE: {}", filename, row.get::<_,String>(1)?),
                            url: format!("local://{}/notes/{}", filename, row.get::<_,String>(1)?),
                            content: format!("(Summary updated: {}) {}", row.get::<_,String>(2)?, row.get::<_,String>(0)?),
                            engine: "LocalDB".into()
                        })
                    });
                    if let Ok(iter) = notes_rows { for r in iter.flatten() { results.push(r); } }
                }

                // 2. Search Messages (Deep Search)
                // Strategy: Fetch many hits sorted by DATE (newest first), then Deduplicate by Conversation
                let has_fts: bool = conn.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='messages_fts'", 
                    [], |r| r.get(0)
                ).unwrap_or(false);

                // Use simple struct to hold raw hits before fetching full context
                struct RawHit { id: i64, conv_id: i64, date: String }

                let sql = if has_fts {
                    // Join FTS with Messages to get Created_At for sorting
                    "SELECT m.id, m.conversation_id, m.created_at 
                     FROM messages_fts f 
                     JOIN messages m ON f.rowid = m.id 
                     WHERE messages_fts MATCH ? 
                     ORDER BY m.created_at DESC 
                     LIMIT ?"
                } else {
                    "SELECT id, conversation_id, created_at 
                     FROM messages 
                     WHERE content LIKE '%' || ? || '%' 
                     ORDER BY created_at DESC 
                     LIMIT ?"
                };

                // Remove quotes for broader FTS match
                let param = if has_fts { query.replace("\"", "") } else { query.clone() };

                let mut raw_hits = Vec::new();
                if let Ok(mut stmt) = conn.prepare(sql) {
                    let rows = stmt.query_map(params![param, limit_raw_hits], |row| {
                        Ok(RawHit { 
                            id: row.get(0)?, 
                            conv_id: row.get(1)?, 
                            date: row.get(2)? 
                        })
                    });
                    if let Ok(iter) = rows {
                        for r in iter.flatten() { raw_hits.push(r); }
                    }
                }

                // Filter Logic: Ensure diversity by taking only 1 hit per conversation
                let mut seen_convs = HashSet::new();
                let mut diverse_hits = Vec::new();
                
                for hit in raw_hits {
                    if !seen_convs.contains(&hit.conv_id) {
                        seen_convs.insert(hit.conv_id);
                        diverse_hits.push(hit);
                    }
                }

                // 3. Fetch Context for Selected Hits
                if !diverse_hits.is_empty() {
                     if let Ok(mut context_stmt) = conn.prepare(
                        "SELECT role, content, created_at FROM messages 
                         WHERE conversation_id = ? AND id >= ? - 3 AND id <= ? + 3
                         ORDER BY id ASC"
                    ) {
                        if let Ok(mut title_stmt) = conn.prepare("SELECT title FROM conversations WHERE id = ?") {
                            
                            for hit in diverse_hits {
                                let chat_title: String = title_stmt.query_row(params![hit.conv_id], |r| r.get(0)).unwrap_or("Chat".into());
                                
                                let rows = context_stmt.query_map(params![hit.conv_id, hit.id, hit.id], |row| {
                                    let role: String = row.get(0)?;
                                    let content: String = row.get(1)?;
                                    let date: String = row.get(2)?;
                                    Ok(format!("({}) {}: {}", date, role.to_uppercase(), content))
                                });

                                if let Ok(msgs) = rows {
                                    let full_transcript = msgs.filter_map(|r| r.ok()).collect::<Vec<_>>().join("\n\n");
                                    
                                    results.push(SearchResult {
                                        title: format!("[Local: {}] Chat: {}", filename, chat_title),
                                        url: format!("local://{}/chat/{}/{}", filename, chat_title, hit.id), 
                                        content: full_transcript,
                                        engine: "LocalDB".into()
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        results
    });

    match task.await {
        Ok(res) => res,
        Err(_) => vec![]
    }
}

async fn searxng_search(client: Client, query: String, timeframe: Option<String>) -> Vec<SearchResult> {
    let base = std::env::var("SEARXNG_URL").unwrap_or_default();
    if base.is_empty() { return vec![]; }
    let mut url = format!("{}/search?q={}&format=json", base, urlencoding::encode(&query));
    if let Some(tf) = timeframe {
        if ["day", "week", "month"].contains(&tf.as_str()) { url.push_str(&format!("&time_range={}", tf)); }
    }
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
             if let Some(arr) = json["results"].as_array() {
                 return arr.iter().map(|r| SearchResult{
                     title: r["title"].as_str().unwrap_or("").into(),
                     url: r["url"].as_str().unwrap_or("").into(),
                     content: r["content"].as_str().unwrap_or("").into(),
                     engine: "SearXNG".into()
                 }).collect();
             }
        }
    }
    vec![]
}

async fn ddg_web(client: Client, q: String, timeframe: Option<String>) -> Vec<SearchResult> {
    let mut url = format!("https://duckduckgo.com/html/?q={}&kp=1", urlencoding::encode(&q));
    if let Some(tf) = timeframe {
        let df = match tf.as_str() { "day" => "d", "week" => "w", "month" => "m", _ => "" };
        if !df.is_empty() { url.push_str(&format!("&df={}", df)); }
    }
    if let Ok(resp) = client.get(&url).send().await {
        let html = resp.text().await.unwrap_or_default();
        let doc = Html::parse_document(&html);
        let res_sel = Selector::parse(".result").unwrap();
        let a_sel = Selector::parse("a.result__a").unwrap();
        let s_sel = Selector::parse(".result__snippet").unwrap();
        let mut out = Vec::new();
        for el in doc.select(&res_sel) {
            if let Some(a) = el.select(&a_sel).next() {
                out.push(SearchResult {
                    title: a.text().collect::<String>().trim().into(),
                    url: a.value().attr("href").unwrap_or("").into(),
                    content: el.select(&s_sel).next().map(|s| s.text().collect::<String>()).unwrap_or_default().trim().into(),
                    engine: "DuckDuckGo".into()
                });
            }
        }
        out
    } else { vec![] }
}

async fn qwant_web(client: Client, q: String) -> Vec<SearchResult> {
    let url = format!("https://www.qwant.com/?q={}&t=web", urlencoding::encode(&q));
    match client.get(&url).send().await {
        Ok(resp) => {
            let html = resp.text().await.unwrap_or_default();
            let fragment = Html::parse_document(&html);
            let result_sel = Selector::parse("[data-testid=\"result-card\"]").unwrap();
            let mut out = Vec::new();
            for el in fragment.select(&result_sel) {
                 let link_sel = Selector::parse("a").unwrap();
                 if let Some(a) = el.select(&link_sel).next() {
                     let title = a.text().collect::<String>().trim().to_string();
                     let url = a.value().attr("href").unwrap_or("").to_string();
                     if !url.is_empty() {
                         out.push(SearchResult { title, url, content: "Qwant Result".into(), engine: "Qwant".into() });
                     }
                 }
            }
            out
        },
        Err(_) => vec![]
    }
}

async fn mojeek_web(client: Client, q: String) -> Vec<SearchResult> {
    let url = format!("https://www.mojeek.com/search?q={}", urlencoding::encode(&q));
    if let Ok(resp) = client.get(&url).send().await {
        let html = resp.text().await.unwrap_or_default();
        let doc = Html::parse_document(&html);
        let sel = Selector::parse("div.results div.result").unwrap();
        let mut out = Vec::new();
        for el in doc.select(&sel) {
            if let Some(a) = el.select(&Selector::parse("a").unwrap()).next() {
                out.push(SearchResult {
                    title: a.text().collect::<String>().trim().into(),
                    url: a.value().attr("href").unwrap_or("").into(),
                    content: el.select(&Selector::parse("p.s").unwrap()).next().map(|s| s.text().collect::<String>()).unwrap_or_default(),
                    engine: "Mojeek".into()
                });
            }
        }
        out
    } else { vec![] }
}

async fn wikipedia_web(client: Client, q: String) -> Vec<SearchResult> {
    let url = format!("https://en.wikipedia.org/w/api.php?action=query&list=search&utf8=1&format=json&srsearch={}", urlencoding::encode(&q));
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = json["query"]["search"].as_array() {
                return arr.iter().map(|i| SearchResult{
                    title: i["title"].as_str().unwrap_or("").into(),
                    url: format!("https://en.wikipedia.org/wiki/{}", i["title"].as_str().unwrap_or("").replace(" ","_")),
                    content: i["snippet"].as_str().unwrap_or("").replace("<span class=\"searchmatch\">","").replace("</span>",""),
                    engine: "Wikipedia".into()
                }).collect();
            }
        }
    }
    vec![]
}

async fn reddit_web(client: Client, q: String) -> Vec<SearchResult> {
    let url = format!("https://www.reddit.com/search.json?q={}&sort=relevance&limit=10", urlencoding::encode(&q));
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = json["data"]["children"].as_array() {
                return arr.iter().map(|c| SearchResult{
                    title: c["data"]["title"].as_str().unwrap_or("").into(),
                    url: format!("https://www.reddit.com{}", c["data"]["permalink"].as_str().unwrap_or("")),
                    content: c["data"]["selftext"].as_str().unwrap_or("").chars().take(200).collect(),
                    engine: "Reddit".into()
                }).collect();
            }
        }
    }
    vec![]
}

async fn stackexchange_web(client: Client, q: String) -> Vec<SearchResult> {
    let url = format!("https://api.stackexchange.com/2.3/search/advanced?order=desc&sort=relevance&q={}&site=stackoverflow", urlencoding::encode(&q));
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = json["items"].as_array() {
                return arr.iter().map(|i| SearchResult{
                    title: i["title"].as_str().unwrap_or("").into(),
                    url: i["link"].as_str().unwrap_or("").into(),
                    content: format!("Score: {}", i["score"]),
                    engine: "StackOverflow".into()
                }).collect();
            }
        }
    }
    vec![]
}

pub async fn suggest(Query(p): Query<std::collections::HashMap<String,String>>) -> Json<Vec<String>> {
    let q = p.get("q").cloned().unwrap_or_default();
    if q.is_empty() { return Json(vec![]); }
    let url = format!("https://duckduckgo.com/ac/?type=list&q={}", q);
    let client = Client::new();
    if let Ok(resp) = client.get(&url).send().await {
         if let Ok(json) = resp.json::<serde_json::Value>().await {
             if let Some(arr) = json[1].as_array() {
                 return Json(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
             }
         }
    }
    Json(vec![])
}