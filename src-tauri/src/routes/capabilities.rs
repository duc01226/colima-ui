use crate::api_server::{ok, ApiResponse};
use axum::{http::StatusCode, response::Json};
use serde_json::json;

pub async fn api_capabilities() -> (StatusCode, Json<ApiResponse<serde_json::Value>>) {
    let mut caps = json!({
        "version": "1.0",
        "description": "ColimaUI capabilities schema for AI Agents",
        "endpoints": {
            "/api/capabilities": "GET - Fetch this schema",
            "/api/sandbox/execute": "POST - Execute shell commands safely",
            "/api/sandbox/execute-approved": "POST - Execute approved shell commands",
            "/api/cli/execute_stream": "POST - Execute safe-only commands; bounded completed output via SSE",
            "/api/kb/query": "POST - Query knowledge bank",
            "/api/kb/search": "POST - Search user memories",
            "/api/settings": "GET, POST - Read/write settings",
            "/api/instances": "GET - List Colima instances",
            "/api/instances/status": "GET - Get status of an instance",
            "/api/instances/start": "POST - Start an instance",
            "/api/instances/stop": "POST - Stop an instance",
            "/api/containers": "GET - List Docker containers",
            "/api/ai/chat": "POST - Chat with configured LLM",
        },
        "event_bus": {
            "colima-start": { "category": "DANGEROUS", "description": "Start colima instance" },
            "colima-stop": { "category": "DANGEROUS", "description": "Stop colima instance" },
            "docker-list": { "category": "SAFE", "description": "List docker containers" },
            "docker-stop": { "category": "NORMAL", "description": "Stop docker container" },
            "k8s-get-pods": { "category": "SAFE", "description": "Get K8s pods" },
            "system-specs": { "category": "SAFE", "description": "Get host specs" }
        }
    });

    if cfg!(windows) {
        if let Some(endpoints) = caps["endpoints"].as_object_mut() {
            endpoints.retain(|path, _| !path.starts_with("/api/instances"));
        }
        if let Some(events) = caps["event_bus"].as_object_mut() {
            events.retain(|name, _| !name.starts_with("colima-") && !name.starts_with("k8s-"));
        }
    }
    ok(caps)
}
