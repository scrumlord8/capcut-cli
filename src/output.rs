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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_envelope_has_ok_status() {
        let env = success("test-cmd", serde_json::json!({"key": "val"}), None);
        assert_eq!(env.status, "ok");
        assert_eq!(env.command, "test-cmd");
        assert!(env.errors.is_empty());
        assert_eq!(env.data["key"], "val");
    }

    #[test]
    fn success_envelope_includes_duration_when_provided() {
        let start = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let env = success("cmd", serde_json::json!(null), Some(start));
        assert!(env.meta.duration_ms.unwrap() >= 5);
    }

    #[test]
    fn success_envelope_omits_duration_when_none() {
        let env = success("cmd", serde_json::json!(null), None);
        assert!(env.meta.duration_ms.is_none());
        // Serialized JSON should not contain duration_ms
        let json = serde_json::to_value(&env).unwrap();
        assert!(json["meta"].get("duration_ms").is_none());
    }

    #[test]
    fn error_envelope_has_error_status_and_entries() {
        let env = error("bad-cmd", "ERR_CODE", "something broke", Some("try X"));
        assert_eq!(env.status, "error");
        assert_eq!(env.command, "bad-cmd");
        assert_eq!(env.data, serde_json::Value::Null);
        assert_eq!(env.errors.len(), 1);
        assert_eq!(env.errors[0].code, "ERR_CODE");
        assert_eq!(env.errors[0].message, "something broke");
        assert_eq!(env.errors[0].hint.as_deref(), Some("try X"));
    }

    #[test]
    fn error_envelope_omits_hint_when_none() {
        let env = error("cmd", "CODE", "msg", None);
        assert!(env.errors[0].hint.is_none());
        let json = serde_json::to_value(&env).unwrap();
        assert!(json["errors"][0].get("hint").is_none());
    }

    #[test]
    fn envelope_serializes_to_valid_json() {
        let env = success("library import", serde_json::json!({"id": "snd_123"}), None);
        let json = serde_json::to_string_pretty(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["command"], "library import");
        assert_eq!(parsed["data"]["id"], "snd_123");
        assert!(parsed["meta"]["version"].is_string());
    }
}
