//! The access record under `pnpr::access` carries each field once, in both
//! log formats.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use pnpr::{LogConfig, LogFormat, logging::log_subscriber, router};
use std::{
    io,
    sync::{Arc, Mutex},
};
use tempfile::TempDir;
use tower::ServiceExt;
use tracing_subscriber::EnvFilter;

use crate::registry_groups;

const ACCESS_FIELDS: [&str; 9] = [
    "request_id",
    "method",
    "uri",
    "status",
    "latency_ms",
    "bytes",
    "client_ip",
    "user",
    "user_agent",
];

#[derive(Debug, Default, Clone)]
struct CapturedOutput(Arc<Mutex<Vec<u8>>>);

impl io::Write for CapturedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl CapturedOutput {
    fn access_record(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let text = String::from_utf8(bytes.clone()).unwrap();
        let records: Vec<&str> = text
            .lines()
            .filter(|line| line.contains("pnpr::access"))
            .collect();
        assert_eq!(records.len(), 1, "one access record per request in:\n{text}");
        records[0].to_string()
    }
}

async fn ping_under(format: LogFormat) -> (StatusCode, String) {
    let tmp = TempDir::new().unwrap();
    let config = registry_groups::grouped_config(tmp.path(), "$all");
    let output = CapturedOutput::default();
    let writer = {
        let output = output.clone();
        move || output.clone()
    };
    let logs = LogConfig { format, ..LogConfig::default() };
    let subscriber = log_subscriber(&logs, EnvFilter::new("info"), writer, false);
    let _default = tracing::subscriber::set_default(subscriber);

    let request = Request::builder()
        .method("GET")
        .uri("/-/ping?valid=7")
        .header("user-agent", "curl/8.18.0")
        .body(Body::empty())
        .unwrap();
    let response = router(config).oneshot(request).await.unwrap();
    (response.status(), output.access_record())
}

#[tokio::test]
async fn pretty_access_record_prints_each_field_once() {
    let (status, record) = ping_under(LogFormat::Pretty).await;

    assert!(record.contains("finished processing request"), "{record}");
    for field in ACCESS_FIELDS {
        let occurrences = record
            .matches(&format!(" {field}="))
            .count();
        assert_eq!(occurrences, 1, "{field} appears once in: {record}");
    }
    assert!(record.contains(" method=GET "), "{record}");
    assert!(record.contains(" uri=/-/ping?valid=7 "), "{record}");
    assert!(record.contains(&format!(" status={} ", status.as_u16())), "{record}");
    assert!(record.contains(" user=- "), "{record}");
    assert!(record.ends_with(" user_agent=curl/8.18.0"), "{record}");
}

#[tokio::test]
async fn json_access_record_is_one_flat_object() {
    let (status, record) = ping_under(LogFormat::Json).await;

    let record: serde_json::Value = serde_json::from_str(&record).unwrap();
    assert_eq!(record["target"], "pnpr::access");
    assert_eq!(record["fields"]["message"], "finished processing request");
    for field in ACCESS_FIELDS {
        assert!(record["fields"].get(field).is_some(), "{field} in: {record}");
    }
    assert_eq!(record["fields"]["method"], "GET");
    assert_eq!(record["fields"]["uri"], "/-/ping?valid=7");
    assert_eq!(record["fields"]["status"], u64::from(status.as_u16()));
    assert_eq!(record["fields"]["user"], "-");
    assert_eq!(record["fields"]["user_agent"], "curl/8.18.0");
    assert!(record.get("span").is_none(), "no span object repeating the fields: {record}");
    assert!(record.get("spans").is_none(), "no span list: {record}");
}
