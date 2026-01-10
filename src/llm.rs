use std::time::Duration;
use serde_json::json;
use reqwest::Client;
use log::{info, error as log_error};

use crate::bot_config::ConfigParamsLlm;
use crate::error::Error;

pub async fn generate_comment(config: &ConfigParamsLlm, image_url: &str) -> Result<String, Error> {
    info!("Generating comment for image (length/url): {}", image_url.len());
    let client = Client::builder()
        .timeout(Duration::from_secs(300)) // 5 minutes timeout for local LLMs
        .build()
        .map_err(|e| error!(LlmError, format!("Failed to build client: {}", e)))?;

    // If no model is specified in config, fetch the first one from the /models endpoint.
    let model = match &config.model {
        Some(m) => m.clone(),
        None => {
            let models_url = if config.api_base.ends_with("/chat/completions") {
                config.api_base.replace("/chat/completions", "/models")
            } else {
                format!("{}/models", config.api_base.trim_end_matches('/'))
            };

            info!("Model not specified, fetching from {}", models_url);
            let res = client.get(models_url)
                .header("Authorization", format!("Bearer {}", config.api_key))
                .send()
                .await
                .map_err(|e| error!(LlmError, format!("Failed to fetch models: {}", e)))?;

            let list: serde_json::Value = res.json().await
                .map_err(|e| error!(LlmError, format!("Failed to parse models list: {}", e)))?;

            list["data"][0]["id"].as_str()
                .ok_or_else(|| error!(LlmError, "No models found in the API response"))?
                .to_string()
        }
    };

    info!("Using model: {}", model);

    // Prepare OpenAI-compatible payload with the image (URL or Base64 data).
    let payload = json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": config.system_prompt
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": image_url
                        }
                    }
                ]
            }
        ]
    });

    let url = if config.api_base.ends_with("/chat/completions") {
        config.api_base.clone()
    } else {
        format!("{}/chat/completions", config.api_base.trim_end_matches('/'))
    };

    let res = client.post(url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| error!(LlmError, format!("Failed to send request: {}", e)))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(error!(LlmError, format!("API returned error {}: {}", status, text)));
    }

    let response_body: serde_json::Value = res.json().await
        .map_err(|e| error!(LlmError, format!("Failed to parse response: {}", e)))?;

    let content = response_body["choices"][0]["message"]["content"].as_str()
        .ok_or_else(|| error!(LlmError, "Response format invalid: missing content"))?;

    Ok(content.to_string())
}
