//! The warnings a `publishConfig.variants` declaration earns.

use super::{Host, PackOptions, fixture, touch};
use crate::pack_archives;
use pnpm_reporter::{LogEvent, LogLevel, Reporter};
use serde_json::{Value, json};
use std::sync::Mutex;

/// Pack the package at `opts.dir` with `Recorder`, and give the warnings `events`
/// recorded.
fn warnings<Recorder: Reporter>(opts: &PackOptions, events: &Mutex<Vec<LogEvent>>) -> Vec<String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(pack_archives::<Recorder, Host>(opts, &[]))
        .unwrap();
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

fn declaration(files: Option<Value>, variants: &Value) -> Value {
    let mut manifest = json!({
        "name": "example-cli",
        "version": "4.0.0",
        "publishConfig": { "variants": variants },
    });
    if let Some(files) = files {
        manifest["files"] = files;
    }
    manifest
}

macro_rules! recording_reporter {
    ($reporter:ident, $events:ident) => {
        static $events: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
        struct $reporter;
        impl Reporter for $reporter {
            fn emit(event: &LogEvent) {
                $events
                    .lock()
                    .unwrap()
                    .push(event.clone());
            }
        }
    };
}

#[test]
fn a_declaration_with_an_any_entry_and_files_earns_no_warning() {
    recording_reporter!(Recording, EVENTS);
    let (dir, opts) = fixture(&declaration(
        Some(json!([])),
        &json!([
            { "targets": ["any"], "files": ["lib.js"] },
            { "targets": ["linux-x64"], "files": ["native/cli"] },
        ]),
    ));
    touch(dir.path(), "lib.js", "");
    touch(dir.path(), "native/cli", "");
    assert_eq!(warnings::<Recording>(&opts, &EVENTS), Vec::<String>::new());
}

#[test]
fn a_declaration_without_an_any_entry_warns() {
    recording_reporter!(Recording, EVENTS);
    let (dir, opts) = fixture(&declaration(
        Some(json!([])),
        &json!([{ "targets": ["linux-x64"], "files": ["native/cli"] }]),
    ));
    touch(dir.path(), "native/cli", "");
    let warnings = warnings::<Recording>(&opts, &EVENTS);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains(r#"no entry that targets "any""#), "{}", warnings[0]);
}

#[test]
fn platform_variants_without_files_warn() {
    recording_reporter!(Recording, EVENTS);
    let (dir, opts) = fixture(&declaration(
        None,
        &json!([
            { "targets": ["any"], "files": ["lib.js"] },
            { "targets": ["linux-x64"], "files": ["native/cli"] },
        ]),
    ));
    touch(dir.path(), "lib.js", "");
    touch(dir.path(), "native/cli", "");
    let warnings = warnings::<Recording>(&opts, &EVENTS);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains(r#"has no "files" field"#), "{}", warnings[0]);
}

/// A `files` that is not an array lists nothing, as no `files` does.
#[test]
fn platform_variants_with_a_files_that_is_no_list_warn() {
    recording_reporter!(Recording, EVENTS);
    let (dir, opts) = fixture(&declaration(
        Some(Value::Null),
        &json!([
            { "targets": ["any"], "files": ["lib.js"] },
            { "targets": ["linux-x64"], "files": ["native/cli"] },
        ]),
    ));
    touch(dir.path(), "lib.js", "");
    touch(dir.path(), "native/cli", "");
    let warnings = warnings::<Recording>(&opts, &EVENTS);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}

/// An `any` entry alone ships no file in two archives, so it needs no
/// `files`.
#[test]
fn an_any_entry_alone_needs_no_files() {
    recording_reporter!(Recording, EVENTS);
    let (dir, opts) =
        fixture(&declaration(None, &json!([{ "targets": ["any"], "files": ["lib.js"] }])));
    touch(dir.path(), "lib.js", "");
    assert_eq!(warnings::<Recording>(&opts, &EVENTS), Vec::<String>::new());
}
