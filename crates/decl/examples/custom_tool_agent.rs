//! Demonstrates loading an Agent from a JSON file and augmenting it
//! with a custom tool that calls an external HTTP API.
//!
//! Run: `cargo run --example custom_tool_agent`
//!
//! This example registers a `weather_lookup` custom tool that calls
//! the Open-Meteo free weather API.

use std::sync::Arc;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;
use rust_agent_core::{ChatMessage, Content, ITool};
use rust_agent_decl::{
    AgentDecl, DefaultAgentResolver,
    resolver::AgentResolver,
};

// ── Step 1: Define a custom tool (external API caller) ──

struct WeatherTool;

#[async_trait::async_trait]
impl ITool for WeatherTool {
    fn name(&self) -> &str {
        "weather_lookup"
    }

    fn description(&self) -> &str {
        "Get current weather for a city. Returns temperature, wind speed, and conditions."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string",
                    "description": "City name, e.g. 'Beijing' or 'London'"
                }
            },
            "required": ["city"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> rust_agent_core::Result<String> {
        let city = arguments["city"].as_str().unwrap_or("Beijing");

        // Geocode city name -> coordinates
        let geo_url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en",
            city
        );

        let client = reqwest::Client::new();
        let geo_resp: serde_json::Value = client
            .get(&geo_url)
            .send()
            .await
            .map_err(|e| rust_agent_core::AgentError::ToolError(format!("Geocoding failed: {}", e)))?
            .json()
            .await
            .map_err(|e| rust_agent_core::AgentError::ToolError(format!("Geo parse failed: {}", e)))?;

        let results = geo_resp["results"].as_array().ok_or_else(|| {
            rust_agent_core::AgentError::ToolError(format!("City '{}' not found", city))
        })?;

        let first = &results[0];
        let lat = first["latitude"].as_f64().unwrap_or(0.0);
        let lon = first["longitude"].as_f64().unwrap_or(0.0);

        // Fetch weather
        let weather_url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current_weather=true",
            lat, lon
        );

        let weather_resp: serde_json::Value = client
            .get(&weather_url)
            .send()
            .await
            .map_err(|e| rust_agent_core::AgentError::ToolError(format!("Weather failed: {}", e)))?
            .json()
            .await
            .map_err(|e| rust_agent_core::AgentError::ToolError(format!("Weather parse: {}", e)))?;

        let current = &weather_resp["current_weather"];
        let temp = current["temperature"].as_f64().unwrap_or(0.0);
        let wind = current["windspeed"].as_f64().unwrap_or(0.0);
        let code = current["weathercode"].as_u64().unwrap_or(0);

        let condition = match code {
            0 => "Clear sky",
            1..=3 => "Partly cloudy",
            45 | 48 => "Foggy",
            51..=55 => "Drizzle",
            61..=65 => "Rain",
            71..=77 => "Snow",
            80..=82 => "Rain showers",
            95..=99 => "Thunderstorm",
            _ => "Unknown",
        };

        Ok(serde_json::json!({
            "ok": true,
            "data": {
                "city": city,
                "temperature_celsius": temp,
                "wind_speed_kmh": wind,
                "condition": condition
            }
        }).to_string())
    }
}

// ── Step 2: Agent declaration (inline JSON) ──

const AGENT_JSON: &str = r#"{
    "id": "weather-assistant",
    "description": "A weather-aware assistant",
    "instructions": "You are a helpful weather assistant. Use the weather_lookup tool to get current weather data. Report temperature, wind, and conditions clearly.",
    "model": {
        "provider": "openai",
        "model": "gpt-4o-mini",
        "api_key": "$OPENAI_API_KEY"
    },
    "tools": [
        { "type": "builtin", "name": "web_search" }
    ],
    "max_tool_rounds": 5
}"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Declarative Agent + Custom Tool Demo ===\n");

    // Parse agent from JSON
    println!("[1] Loading agent from JSON...");
    let decl = AgentDecl::from_json_str(AGENT_JSON)?;
    println!("    id={}, model={}/{}", decl.id, decl.model.provider, decl.model.model);

    // Register custom tool
    println!("\n[2] Registering custom 'weather_lookup' tool...");
    let mut resolver = DefaultAgentResolver::new();
    resolver.register_tool_factory("weather_lookup", |_config| {
        Ok(Arc::new(WeatherTool))
    });

    // Build agent
    println!("\n[3] Building agent...");
    let agent = resolver.resolve(&decl).await?;
    println!("    Agent '{}' built successfully", agent.id());

    // Test custom tool directly
    println!("\n[4] Testing weather_lookup directly (Beijing)...");
    let result = WeatherTool
        .execute(serde_json::json!({"city": "Beijing"}))
        .await?;
    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    if parsed["ok"] == true {
        let d = &parsed["data"];
        println!("    {}: {}°C, wind {} km/h, {}",
            d["city"], d["temperature_celsius"], d["wind_speed_kmh"], d["condition"]);
    }

    // Run agent with LLM
    println!("\n[5] Running agent (requires OPENAI_API_KEY)...");
    match std::env::var("OPENAI_API_KEY") {
        Ok(_) => {
            let messages = vec![ChatMessage::user("What's the weather like in Tokyo?")];
            let session = agent.create_session();
            let mut stream = agent.run(messages, Some(session), None).await?;

            print!("    ");
            loop {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(120),
                    poll_next(&mut stream),
                ).await {
                    Ok(Some(Ok(result))) => {
                        for content in &result.contents {
                            if let Content::Text(t) = content {
                                print!("{}", t.delta);
                            }
                        }
                        if result.finish_reason.is_some() {
                            println!();
                            break;
                        }
                    }
                    Ok(Some(Err(e))) => {
                        eprintln!("\n    [Error]: {}", e);
                        break;
                    }
                    Ok(None) => break,
                    Err(_) => {
                        eprintln!("\n    [Timeout]");
                        break;
                    }
                }
            }
        }
        Err(_) => {
            println!("    (skipped — set OPENAI_API_KEY to test)");
            println!("\n    Tip: You can also use DeepSeek:");
            println!("      Change provider to \"deepseek\" and api_key to \"$DEEPSEEK_API_KEY\"");
        }
    }

    println!("\n=== Done ===");
    Ok(())
}

/// Helper: poll the next item from a boxed stream.
async fn poll_next<T: Unpin>(
    stream: &mut (dyn Stream<Item = T> + Unpin + Send),
) -> Option<T> {
    StreamNext { stream }.await
}

struct StreamNext<'a, T> {
    stream: &'a mut (dyn Stream<Item = T> + Unpin + Send),
}

impl<T: Unpin> std::future::Future for StreamNext<'_, T> {
    type Output = Option<T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.stream).poll_next(cx)
    }
}
