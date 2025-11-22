use serde::Serialize;
use scraper::{Html, Selector};
use axum::{Json, extract::Query};
use std::collections::HashSet;
use futures::future::join_all;
use reqwest::Client;
use std::pin::Pin;
use std::future::Future;

#[derive(Serialize, Clone, Debug)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub engine: String,
}

// --- Controller for Autocomplete ---
pub async fn suggest(Query(params): Query<std::collections::HashMap<String, String>>) -> Json<Vec<String>> {
    let q = params.get("q").cloned().unwrap_or_default();
    if q.trim().is_empty() { return Json(vec![]); }

    let client = Client::new();
    // We clone client here too, just to be consistent and safe, though strict lifetimes might handle it.
    let futs = vec![
        fetch_suggest(client.clone(), format!("https://duckduckgo.com/ac/?type=list&q={}", q), "ddg"),
        fetch_suggest(client.clone(), format!("https://search.brave.com/api/suggest?q={}", q), "brave"),
        fetch_suggest(client.clone(), format!("https://api.qwant.com/v3/suggest?q={}&locale=en_US&version=2", q), "qwant"),
        fetch_suggest(client.clone(), format!("https://en.wikipedia.org/w/api.php?action=opensearch&format=json&formatversion=2&namespace=0&limit=10&search={}", q), "wiki"),
    ];

    let results = join_all(futs).await;
    let mut all_suggestions = Vec::new();
    for res in results {
        all_suggestions.extend(res);
    }

    // Frequency count / De-dup
    let mut counts = std::collections::HashMap::new();
    for s in all_suggestions {
        *counts.entry(s).or_insert(0) += 1;
    }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by frequency

    Json(sorted.into_iter().take(10).map(|(s, _)| s).collect())
}

async fn fetch_suggest(client: Client, url: String, source: &str) -> Vec<String> {
    let res = client.get(&url).header("User-Agent", "bplus-native/1.0").send().await;
    if let Ok(resp) = res {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            match source {
                "ddg" | "wiki" => {
                    if let Some(arr) = json.as_array().and_then(|a| a.get(1)).and_then(|v| v.as_array()) {
                        return arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                    }
                },
                "brave" => {
                     if let Some(arr) = json.get(1).and_then(|v| v.as_array()) {
                         return arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                     }
                },
                "qwant" => {
                    if let Some(items) = json.get("data").and_then(|d| d.get("items")).and_then(|i| i.as_array()) {
                        return items.iter().filter_map(|v| v.get("value").and_then(|s| s.as_str().map(String::from))).collect();
                    }
                },
                _ => {}
            }
        }
    }
    vec![]
}

// --- Main Search Functions ---

pub async fn searxng_search(query: &str, timeframe: Option<&str>) -> Vec<SearchResult> {
    let base_url = std::env::var("SEARXNG_URL").expect("SEARXNG_URL not set");
    let mut url = format!("{}/search?q={}&format=json", base_url, urlencoding::encode(query));
    if let Some(tf) = timeframe {
        if ["day", "week", "month"].contains(&tf) {
            url.push_str(&format!("&time_range={}", tf));
        }
    }

    let client = Client::new();
    let mut req = client.get(&url);
    
    // Add Auth if present
    if let (Ok(user), Ok(pass)) = (std::env::var("AUTH_USERNAME"), std::env::var("AUTH_PASSWORD")) {
        req = req.basic_auth(user, Some(pass));
    }

    match req.send().await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(results) = json.get("results").and_then(|r| r.as_array()) {
                    return results.iter().map(|r| SearchResult {
                        title: r["title"].as_str().unwrap_or("").to_string(),
                        url: r["url"].as_str().unwrap_or("").to_string(),
                        content: r["content"].as_str().unwrap_or("").to_string(),
                        engine: "searxng".to_string(),
                    }).collect();
                }
            }
        }
        Err(e) => eprintln!("SearXNG Error: {}", e),
    }
    vec![]
}

pub async fn native_search(query: &str, timeframe: Option<&str>) -> Vec<SearchResult> {
    let client = Client::builder()
        .user_agent("bplus-native/1.0")
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .unwrap();

    // Define the type for dynamic dispatch of Futures
    type SearchFut = Pin<Box<dyn Future<Output = Vec<SearchResult>> + Send>>;

    // FIX: Pass client.clone() to each function so they own their handle.
    let tasks: Vec<SearchFut> = vec![
        Box::pin(ddg_web(client.clone(), query.to_string(), timeframe.map(|s| s.to_string()))),
        Box::pin(mojeek_web(client.clone(), query.to_string())),
        Box::pin(qwant_web(client.clone(), query.to_string())),
        Box::pin(wikipedia_web(client.clone(), query.to_string())),
        Box::pin(reddit_web(client.clone(), query.to_string())),
        Box::pin(stackexchange_web(client.clone(), query.to_string())),
    ];

    let results = join_all(tasks).await;
    let mut all_results = Vec::new();
    
    for res in results {
        all_results.extend(res);
    }

    // Deduplicate by URL
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for r in all_results {
        if !seen.contains(&r.url) {
            seen.insert(r.url.clone());
            unique.push(r);
        }
    }

    // Simple sort relevance: Title contains query
    let q_lower = query.to_lowercase();
    unique.sort_by(|a, b| {
        let a_score = if a.title.to_lowercase().contains(&q_lower) { 1 } else { 0 };
        let b_score = if b.title.to_lowercase().contains(&q_lower) { 1 } else { 0 };
        b_score.cmp(&a_score)
    });

    unique
}

// --- Individual Scrapers ---
// Updated all signatures to take `client: Client` (owned) instead of `&Client`

async fn ddg_web(client: Client, q: String, timeframe: Option<String>) -> Vec<SearchResult> {
    let mut url = format!("https://duckduckgo.com/html/?q={}&kp=1", urlencoding::encode(&q));
    if let Some(tf) = timeframe {
        let df = match tf.as_str() { "day" => "d", "week" => "w", "month" => "m", _ => "" };
        if !df.is_empty() { url.push_str(&format!("&df={}", df)); }
    }

    match client.get(&url).send().await {
        Ok(resp) => {
            let html = resp.text().await.unwrap_or_default();
            let fragment = Html::parse_document(&html);
            let result_sel = Selector::parse(".result").unwrap();
            let title_sel = Selector::parse("a.result__a").unwrap();
            let snip_sel = Selector::parse(".result__snippet").unwrap();

            let mut out = Vec::new();
            for el in fragment.select(&result_sel) {
                if let Some(a) = el.select(&title_sel).next() {
                    let title = a.text().collect::<String>().trim().to_string();
                    let url = a.value().attr("href").unwrap_or("").to_string();
                    let content = el.select(&snip_sel).next().map(|s| s.text().collect::<String>()).unwrap_or_default().trim().to_string();
                    if !url.is_empty() {
                        out.push(SearchResult { title, url, content, engine: "duckduckgo".into() });
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
    match client.get(&url).send().await {
        Ok(resp) => {
            let html = resp.text().await.unwrap_or_default();
            let fragment = Html::parse_document(&html);
            let sel = Selector::parse("div.results div.result").unwrap();
            let mut out = Vec::new();
            for el in fragment.select(&sel) {
                let link = el.select(&Selector::parse("a").unwrap()).next();
                if let Some(a) = link {
                    let title = a.text().collect::<String>().trim().to_string();
                    let url = a.value().attr("href").unwrap_or("").to_string();
                    let content = el.select(&Selector::parse("p.s").unwrap()).next().map(|s| s.text().collect::<String>()).unwrap_or_default();
                    if !url.is_empty() {
                        out.push(SearchResult { title, url, content, engine: "mojeek".into() });
                    }
                }
            }
            out
        },
        Err(_) => vec![]
    }
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
                 // Simplified extraction logic for Qwant's dynamic DOM
                 let link_sel = Selector::parse("a").unwrap();
                 if let Some(a) = el.select(&link_sel).next() {
                     let title = a.text().collect::<String>().trim().to_string();
                     let url = a.value().attr("href").unwrap_or("").to_string();
                     if !url.is_empty() {
                         out.push(SearchResult { title, url, content: "Qwant Result".into(), engine: "qwant".into() });
                     }
                 }
            }
            out
        },
        Err(_) => vec![]
    }
}

async fn wikipedia_web(client: Client, q: String) -> Vec<SearchResult> {
    let url = format!("https://en.wikipedia.org/w/api.php?action=query&list=search&utf8=1&format=json&srsearch={}&srlimit=10", urlencoding::encode(&q));
    match client.get(&url).send().await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(items) = json.get("query").and_then(|q| q.get("search")).and_then(|a| a.as_array()) {
                    return items.iter().map(|i| SearchResult {
                        title: i["title"].as_str().unwrap_or("").to_string(),
                        url: format!("https://en.wikipedia.org/wiki/{}", urlencoding::encode(i["title"].as_str().unwrap_or("")).replace("%20", "_")),
                        content: i["snippet"].as_str().unwrap_or("").replace(r#"<span class="searchmatch">"#, "").replace("</span>", ""),
                        engine: "wikipedia".into()
                    }).collect();
                }
            }
        },
        Err(_) => {}
    }
    vec![]
}

async fn reddit_web(client: Client, q: String) -> Vec<SearchResult> {
    let url = format!("https://www.reddit.com/search.json?q={}&sort=relevance&t=all&limit=10", urlencoding::encode(&q));
    match client.get(&url).send().await {
         Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(children) = json.get("data").and_then(|d| d.get("children")).and_then(|c| c.as_array()) {
                     return children.iter().map(|c| {
                         let data = &c["data"];
                         SearchResult {
                             title: data["title"].as_str().unwrap_or("").to_string(),
                             url: format!("https://www.reddit.com{}", data["permalink"].as_str().unwrap_or("")),
                             content: data["selftext"].as_str().unwrap_or("").chars().take(200).collect(),
                             engine: "reddit".into()
                         }
                     }).collect();
                }
            }
         },
         Err(_) => {}
    }
    vec![]
}

async fn stackexchange_web(client: Client, q: String) -> Vec<SearchResult> {
    let url = format!("https://api.stackexchange.com/2.3/search/advanced?order=desc&sort=relevance&accepted=True&answers=1&q={}&site=stackoverflow&filter=default", urlencoding::encode(&q));
    match client.get(&url).send().await {
        Ok(resp) => {
             if let Ok(json) = resp.json::<serde_json::Value>().await {
                 if let Some(items) = json.get("items").and_then(|i| i.as_array()) {
                     return items.iter().map(|i| SearchResult {
                         title: i["title"].as_str().unwrap_or("").to_string(),
                         url: i["link"].as_str().unwrap_or("").to_string(),
                         content: format!("Score: {}", i["score"]),
                         engine: "stackexchange".into()
                     }).collect();
                 }
             }
        },
        Err(_) => {}
    }
    vec![]
}