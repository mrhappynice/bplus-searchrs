use serde::{Deserialize, Serialize};
use axum::{Json, extract::Query};
use std::collections::HashMap;
use reqwest::Client;
use futures::stream::BoxStream;
use futures::{Stream, StreamExt};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct Model {
    pub id: String,
    pub name: String,
}

pub async fn list_models(Query(params): Query<HashMap<String, String>>) -> Json<Vec<Model>> {
    let provider = params.get("provider").map(|s| s.as_str()).unwrap_or("");
    let client = Client::new();

    let (url, headers, processor): (String, HashMap<String, String>, Box<dyn Fn(serde_json::Value) -> Vec<Model> + Send>) = match provider {
        "lmstudio" => {
            let base = std::env::var("LMSTUDIO_API_BASE").unwrap_or_default();
            (
                format!("{}/models", base), 
                HashMap::new(), 
                Box::new(|data| {
                    data["data"].as_array().unwrap_or(&vec![]).iter().map(|m| Model{ 
                        id: m["id"].as_str().unwrap_or("").into(), 
                        name: m["id"].as_str().unwrap_or("").into() 
                    }).collect()
                })
            )
        },
        "openai" => {
            let key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
            let mut h = HashMap::new(); 
            h.insert("Authorization".into(), format!("Bearer {}", key));
            (
                "https://api.openai.com/v1/models".into(), 
                h,
                Box::new(|data| {
                    data["data"].as_array().unwrap_or(&vec![]).iter()
                    .filter(|m| { 
                        let id = m["id"].as_str().unwrap_or(""); 
                        id.starts_with("gpt") || id.starts_with("o1") 
                    })
                    .map(|m| Model{ 
                        id: m["id"].as_str().unwrap_or("").into(), 
                        name: m["id"].as_str().unwrap_or("").into() 
                    }).collect()
                })
            )
        },
        "openrouter" => {
            let key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
            let mut h = HashMap::new(); 
            h.insert("Authorization".into(), format!("Bearer {}", key));
            (
                "https://openrouter.ai/api/v1/models".into(), 
                h,
                Box::new(|data| {
                    // FIX: Removed the OpenAI-specific filter here. 
                    // OpenRouter returns many prefixes (anthropic, google, etc.)
                    data["data"].as_array().unwrap_or(&vec![]).iter()
                    .map(|m| Model{ 
                        id: m["id"].as_str().unwrap_or("").into(), 
                        // OpenRouter provides a "name" field, fallback to "id" if missing
                        name: m["name"].as_str().unwrap_or(m["id"].as_str().unwrap_or("")).into() 
                    }).collect()
                })
            )
        },
        "google" => {
             let key = std::env::var("GOOGLE_API_KEY").unwrap_or_default();
             (
                 format!("https://generativelanguage.googleapis.com/v1beta/models?key={}", key),
                 HashMap::new(),
                 Box::new(|data| {
                    data["models"].as_array().unwrap_or(&vec![]).iter()
                    .filter(|m| {
                        m["supportedGenerationMethods"].as_array()
                            .map(|a| a.iter().any(|x| x == "generateContent"))
                            .unwrap_or(false)
                    })
                    .map(|m| Model{ 
                        id: m["name"].as_str().unwrap_or("").into(), 
                        name: m["displayName"].as_str().unwrap_or("").into() 
                    }).collect()
                 })
             )
        },
        _ => return Json(vec![])
    };

    if url.is_empty() || (url.contains("key=") && url.ends_with("=")) { 
        return Json(vec![]); 
    }

    let mut req = client.get(&url);
    for (k, v) in headers { req = req.header(k, v); }
    
    match req.send().await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                Json(processor(json))
            } else {
                Json(vec![])
            }
        },
        Err(_) => Json(vec![])
    }
}

pub async fn stream_completion(
    provider: &str,
    model: &str,
    system_prompt: &str,
    history: Vec<Message>,
    user_prompt: &str
) -> BoxStream<'static, Result<String, anyhow::Error>> {
    let client = Client::new();
    
    if provider == "google" {
        let api_key = std::env::var("GOOGLE_API_KEY").unwrap_or_default();
        let model_id = model.replace("models/", "");
        let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?key={}", model_id, api_key);
        
        let body = serde_json::json!({
            "contents": [{ "parts": [{ "text": format!("{}\n\n{}", system_prompt, user_prompt) }] }]
        });

        let stream = try_stream_google(client, url, body);
        return Box::pin(stream);
    } else {
        // OpenAI Compatible (Local, OpenRouter, OpenAI)
        let (api_base, api_key) = match provider {
            "openai" => ("https://api.openai.com/v1".to_string(), std::env::var("OPENAI_API_KEY").unwrap_or_default()),
            "openrouter" => ("https://openrouter.ai/api/v1".to_string(), std::env::var("OPENROUTER_API_KEY").unwrap_or_default()),
            _ => (std::env::var("LMSTUDIO_API_BASE").unwrap_or_else(|_| "http://localhost:1234/v1".to_string()), "not-needed".to_string()),
        };

        let mut messages = vec![Message { role: "system".into(), content: system_prompt.into() }];
        messages.extend(history);
        messages.push(Message { role: "user".into(), content: user_prompt.into() });

        let body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": true
        });

        let url = format!("{}/chat/completions", api_base);
        let stream = try_stream_openai(client, url, api_key, body);
        return Box::pin(stream);
    }
}

fn try_stream_openai(client: Client, url: String, key: String, body: serde_json::Value) -> impl Stream<Item = Result<String, anyhow::Error>> {
    async_stream::stream! {
        let mut req = client.post(&url).header("Authorization", format!("Bearer {}", key)).json(&body);
        if url.contains("openrouter") {
            req = req.header("HTTP-Referer", "http://localhost:3001").header("X-Title", "Bplus Search");
        }

        let mut source = match req.send().await {
            Ok(resp) => resp.bytes_stream(),
            Err(e) => { yield Err(anyhow::anyhow!(e)); return; }
        };

        while let Some(item) = source.next().await {
            if let Ok(bytes) = item {
                let chunk_str = String::from_utf8_lossy(&bytes);
                for line in chunk_str.lines() {
                    if line.starts_with("data: ") {
                        let data = line.trim_start_matches("data: ").trim();
                        if data == "[DONE]" { break; }
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                                yield Ok(content.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
}

fn try_stream_google(client: Client, url: String, body: serde_json::Value) -> impl Stream<Item = Result<String, anyhow::Error>> {
    async_stream::stream! {
        let mut source = match client.post(&url).json(&body).send().await {
             Ok(resp) => resp.bytes_stream(),
             Err(e) => { yield Err(anyhow::anyhow!(e)); return; }
        };

        while let Some(item) = source.next().await {
            if let Ok(bytes) = item {
                let s = String::from_utf8_lossy(&bytes);
                if let Some(start) = s.find("\"text\": \"") {
                     let rest = &s[start + 9..];
                     if let Some(end) = rest.find("\"") {
                         yield Ok(rest[..end].replace("\\n", "\n").to_string());
                     }
                }
            }
        }
    }
}