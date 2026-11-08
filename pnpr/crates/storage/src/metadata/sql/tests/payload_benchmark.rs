//! Manual comparison of payload layouts with a warm filesystem cache and cold render caches.

use super::{
    Db, npm, packument,
    schema::{PAYLOAD_TABLES, execute},
    storage, write,
};
use crate::metadata::sql::{
    PackageKey, cache::RenderCache, render, schema, transactions::Freshness,
};
use pnpr_package_name::Ecosystem;
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
enum Layout {
    WithoutRowid,
    Rowid,
}

async fn measure(db: Db, layout: Layout) {
    let mut tables = schema::TABLES.to_string();
    match layout {
        Layout::WithoutRowid => {
            for table in PAYLOAD_TABLES {
                let start = tables
                    .find(&format!("CREATE TABLE IF NOT EXISTS {table} ("))
                    .unwrap();
                let end = start + tables[start..].find(") STRICT;").unwrap();
                tables.replace_range(end..end + ") STRICT;".len(), ") STRICT, WITHOUT ROWID;");
            }
        }
        Layout::Rowid => {}
    }
    let mut statements: Vec<_> = schema::statements(&tables).collect();
    statements.push(schema::MARKER_TABLE.to_string());
    statements.push(format!(
        "INSERT INTO {} (version) VALUES ({})",
        schema::MARKER,
        schema::SCHEMA_VERSION,
    ));
    let statements: Vec<_> = statements
        .iter()
        .map(String::as_str)
        .collect();
    execute(&db, &statements).await;
    let storage = storage(&db);
    let versions: Vec<_> = (0..300)
        .map(|index| format!("1.0.{index}"))
        .collect();
    let versions: Vec<_> = versions
        .iter()
        .map(String::as_str)
        .collect();
    let mut document = packument("sample-payload", &versions);
    document["description"] = json!("package state ".repeat(2048));
    for version in &versions {
        document["versions"][*version]["readme"] = json!("README content\n".repeat(4096));
        document["versions"][*version]["description"] = json!("manifest content ".repeat(256));
    }
    let pending = std::mem::take(document["versions"].as_object_mut().unwrap());
    for chunk in versions.chunks(10) {
        for version in chunk {
            document["versions"][*version] = pending[*version].clone();
        }
        document["dist-tags"]["latest"] = json!(chunk.last().unwrap());
        write(&storage, "sample-payload", &document).await;
    }
    let metadata = storage.sql_metadata();
    let mut times = Vec::new();
    for sample in 0..16 {
        let key = PackageKey {
            catalog: String::new(),
            ecosystem: Ecosystem::Npm,
            name: npm("sample-payload").as_str().to_string(),
        };
        let started = Instant::now();
        let view = metadata
            .read(String::new(), Freshness::Bounded, move |connection, _| {
                render::read_view(connection, &RenderCache::default(), &key)
            })
            .await
            .unwrap()
            .unwrap();
        let elapsed = started.elapsed();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&view.bytes).unwrap(), document);
        if sample > 0 {
            times.push(elapsed);
        }
    }
    times.sort();
    let milliseconds = |duration: Duration| duration.as_secs_f64() * 1000.0;
    eprintln!(
        "{layout:?}: median {:.2} ms, min {:.2} ms, max {:.2} ms ({} reads)",
        milliseconds(times[times.len() / 2]),
        milliseconds(times[0]),
        milliseconds(times[times.len() - 1]),
        times.len(),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "manual payload-layout benchmark"]
async fn payload_layout_on_sqlite() {
    for layout in [Layout::WithoutRowid, Layout::Rowid] {
        measure(Db::sqlite(), layout).await;
    }
}

#[cfg(feature = "backend-libsql")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "manual payload-layout benchmark"]
async fn payload_layout_on_libsql_replica() {
    for layout in [Layout::WithoutRowid, Layout::Rowid] {
        measure(Db::libsql(true).await, layout).await;
    }
}
