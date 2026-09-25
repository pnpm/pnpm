use super::{Host, api, fixture, json, touch};
use crate::dotenv::{is_dotenv_file, named_in_files};
use pnpm_reporter::{LogEvent, LogLevel, Reporter};
use serde_json::Value;
use std::{path::Path, sync::Mutex};

fn warnings(events: &Mutex<Vec<LogEvent>>) -> Vec<String> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            LogEvent::Pnpm(log) if matches!(log.level, LogLevel::Warn) => Some(log.message.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn warns_about_packed_dotenv_files_without_dropping_them() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .unwrap()
                .push(event.clone());
        }
    }
    let (dir, opts) = fixture(&json!({ "name": "foo", "version": "1.0.0" }));
    touch(dir.path(), "index.js", "");
    touch(dir.path(), ".env", "SECRET=1\n");
    touch(dir.path(), "config/.env.local", "SECRET=2\n");
    touch(dir.path(), ".env.example", "SECRET=\n");

    let result = api::<RecordingReporter, Host>(&opts).unwrap();

    assert!(result.contents.contains(&".env".to_string()));
    assert!(result.contents.contains(&"config/.env.local".to_string()));
    let warnings = warnings(&EVENTS);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("\n  .env\n"), "{}", warnings[0]);
    assert!(warnings[0].contains("\n  config/.env.local\n"), "{}", warnings[0]);
    assert!(!warnings[0].contains(".env.example"), "{}", warnings[0]);
}

#[test]
fn dotenv_files_named_in_files_are_packed_without_a_warning() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .unwrap()
                .push(event.clone());
        }
    }
    let (dir, opts) = fixture(&json!({
        "name": "foo",
        "version": "1.0.0",
        "files": ["index.js", ".env"],
    }));
    touch(dir.path(), "index.js", "");
    touch(dir.path(), ".env", "PUBLIC=1\n");

    let result = api::<RecordingReporter, Host>(&opts).unwrap();

    assert!(result.contents.contains(&".env".to_string()));
    assert_eq!(warnings(&EVENTS), Vec::<String>::new());
}

#[test]
fn dotenv_files_are_recognized_by_basename() {
    assert!(is_dotenv_file(".env"));
    assert!(is_dotenv_file("nested/dir/.env.production"));
    assert!(!is_dotenv_file(".env.example"));
    assert!(!is_dotenv_file("config/.env.sample"));
    assert!(!is_dotenv_file(".envrc"));
    assert!(!is_dotenv_file("env"));
}

fn is_named_in_files(path: &str, entries: &[&str]) -> bool {
    let entries: Vec<Value> = entries
        .iter()
        .map(|entry| json!(entry))
        .collect();
    named_in_files(Path::new("/pkg"), &entries)(path)
}

#[test]
fn only_entries_that_name_the_dotenv_file_count_as_listing_it() {
    assert!(is_named_in_files("config/.env", &["./config/.env"]));
    assert!(is_named_in_files("config/.env", &["**/.env"]));
    assert!(is_named_in_files("config/.env.local", &["config/.env*"]));
    assert!(!is_named_in_files("config/.env", &["config"]));
    assert!(!is_named_in_files("config/.env", &["config/**"]));
    assert!(!is_named_in_files("config/.env", &[".env"]));
    assert!(!is_named_in_files(".env", &["dist/.env*"]));
    assert!(!is_named_in_files("node_modules/dep/.env", &["dist/.env*"]));
}
