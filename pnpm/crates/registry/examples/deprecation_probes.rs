//! Repeated deprecation scans of immutable metadata, without manifest hydration.
use pnpm_registry::{MirrorFile, Package, PackageVersions};
use std::{hint::black_box, io::Write, time::Instant};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).map_or("raw", String::as_str);
    let passes: usize = args
        .get(2)
        .map_or(100, |s| s.parse().unwrap());
    let mut fragments = Vec::new();
    let mut spans = Vec::new();
    let mut releases = serde_json::Map::new();
    for i in 0..1000 {
        let version = format!("1.0.{i}");
        let release = serde_json::json!({
            "name": "probe", "version": version,
            "deprecated": if i % 3 == 0 { serde_json::json!("use 2.x") } else { serde_json::json!(false) },
            "dist": { "tarball": "https://registry.example/probe.tgz" },
            "description": "x".repeat(2048),
        });
        let json = serde_json::to_vec(&release).unwrap();
        spans.push((version.clone(), fragments.len() as u64, json.len() as u32));
        fragments.extend(json);
        releases.insert(version, release);
    }
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&fragments).unwrap();
    let mirror = MirrorFile::try_hold(file, usize::MAX).unwrap();
    let versions = if mode.starts_with("file") {
        PackageVersions::from_file_spans(&mirror, spans)
    } else {
        let package: Package = serde_json::from_value(serde_json::json!({
            "name": "probe", "dist-tags": {}, "versions": releases,
        }))
        .unwrap();
        package.versions
    };
    let keys: Vec<_> = versions.keys().cloned().collect();
    let start = Instant::now();
    for _ in 0..passes {
        let filtered;
        let versions = if mode.ends_with("filtered") {
            filtered = versions.filtered(|_| true);
            &filtered
        } else {
            &versions
        };
        let count = keys
            .iter()
            .filter(|v| black_box(&versions).is_deprecated(v))
            .count();
        assert_eq!(black_box(count), 334);
    }
    println!("{:.6}", start.elapsed().as_secs_f64() * 1000.0);
}
