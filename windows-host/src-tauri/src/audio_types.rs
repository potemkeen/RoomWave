//! Shared serialized configuration/state. Preserve field names for UI and saved settings.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub enabled: bool,
    pub source_id: Option<String>,
    pub output_id: Option<String>,
    pub speakers: Vec<u32>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub status: String,
    pub error: Option<String>,
    pub sync_error_ms: Option<f64>,
    pub latency_ms: Option<f64>,
    pub output_ms: Option<f64>,
    pub buffer_margin_ms: Option<f64>,
    pub limiter_gain: Option<f64>,
    pub unavailable: Vec<u32>,
    pub stages: Value,
}
#[derive(Clone, Serialize)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
}
