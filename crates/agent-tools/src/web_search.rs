use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for real-time information. Returns search results with titles, URLs, and snippets."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "num_results": {
                    "type": "integer",
                    "description": "Number of results (default 5, max 10)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: &Value, _working_dir: &str) -> anyhow::Result<String> {
        let query = args["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' argument"))?;
        let num = args["num_results"].as_u64().unwrap_or(5).min(10);

        // Use DuckDuckGo HTML API (no API key needed)
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; CodingAgent/1.0)")
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let resp = client.get(&url).send().await?;
        let html = resp.text().await?;

        // Parse results from HTML
        let mut results = Vec::new();
        let mut count = 0;

        // Simple regex-based extraction
        for (i, line) in html.lines().enumerate() {
            if count >= num as usize {
                break;
            }

            // Look for result links
            if line.contains("result__a") || line.contains("result__title") {
                // Extract title and URL from nearby lines
                let title = extract_between(line, ">", "</a>")
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                if !title.is_empty() && title.len() > 5 {
                    let url = extract_between(line, "href=\"", "\"")
                        .unwrap_or_default()
                        .to_string();

                    // Look for snippet in next few lines
                    let snippet_window: String = html
                        .lines()
                        .skip(i)
                        .take(5)
                        .collect::<Vec<_>>()
                        .join(" ");
                    let snippet = extract_between(&snippet_window, "result__snippet", "</a>")
                        .map(|s| {
                            s.trim_start_matches('>')
                                .trim_start_matches('>')
                                .to_string()
                        })
                        .unwrap_or_default()
                        .chars()
                        .take(200)
                        .collect::<String>();

                    results.push(format!(
                        "{}. {}\n   URL: {}\n   {}",
                        count + 1,
                        title,
                        if url.is_empty() { "N/A" } else { &url },
                        if snippet.is_empty() {
                            "No snippet available"
                        } else {
                            &snippet
                        }
                    ));
                    count += 1;
                }
            }
        }

        if results.is_empty() {
            // Fallback: try to extract any useful text
            Ok(format!(
                "Search results for '{}':\n\nNo structured results found. The search engine may have returned results in a format that couldn't be parsed.\n\nRaw HTML length: {} bytes",
                query,
                html.len()
            ))
        } else {
            Ok(format!(
                "Search results for '{}':\n\n{}",
                query,
                results.join("\n\n")
            ))
        }
    }
}

fn extract_between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_idx = text.find(start)? + start.len();
    let remaining = &text[start_idx..];
    let end_idx = remaining.find(end)?;
    Some(&remaining[..end_idx])
}
