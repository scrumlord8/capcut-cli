use serde::Serialize;
use std::time::Instant;

use crate::config::VERSION;

/// Standard JSON envelope for all CLI output.
#[derive(Debug, Serialize)]
pub struct Envelope {
    pub status: &'static str,
    pub command: String,
    pub data: serde_json::Value,
    pub errors: Vec<ErrorEntry>,
    pub meta: Meta,
}

#[derive(Debug, Serialize)]
pub struct ErrorEntry {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Meta {
    pub version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

pub fn success(command: &str, data: serde_json::Value, start: Option<Instant>) -> Envelope {
    Envelope {
        status: "ok",
        command: command.to_string(),
        data,
        errors: vec![],
        meta: Meta {
            version: VERSION,
            duration_ms: start.map(|s| s.elapsed().as_millis() as u64),
        },
    }
}

pub fn error(command: &str, code: &str, message: &str, hint: Option<&str>) -> Envelope {
    Envelope {
        status: "error",
        command: command.to_string(),
        data: serde_json::Value::Null,
        errors: vec![ErrorEntry {
            code: code.to_string(),
            message: message.to_string(),
            hint: hint.map(|h| h.to_string()),
        }],
        meta: Meta {
            version: VERSION,
            duration_ms: None,
        },
    }
}

pub fn emit(envelope: &Envelope) {
    if let Ok(json) = serde_json::to_string_pretty(envelope) {
        println!("{json}");
    }
}

pub fn log(msg: &str) {
    eprintln!("{msg}");
}
