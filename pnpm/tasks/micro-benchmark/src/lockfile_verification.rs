use std::hint::black_box;

use criterion::{
    Criterion,
    Throughput,
};
use pnpm_lockfile::{
    Lockfile,
    ProjectSnapshot,
};
use pnpm_lockfile_verification::verify_lockfile_local_resolutions;
use serde_json::json;

const IMPORTERS: usize = 13_800;
const PACKAGES: usize = 256;

/// A workspace link activates containment. Its per-importer walk should prune
/// the shared registry chain because none of those snapshots references a local package.
fn many_importers() -> Lockfile {
    let mut document = json!({
        "lockfileVersion": "9.0",
        "importers": { ".": { "dependencies": {
            "project-0": { "specifier": "workspace:*", "version": "link:projects/project-0" }
        } } },
        "packages": {},
        "snapshots": {}
    });
    for index in 0..PACKAGES {
        let key = format!("dep-{index}@1.0.0");
        document["packages"][&key] = json!({ "resolution": { "integrity": "sha512-deadbeef" } });
        document["snapshots"][&key] = if index + 1 == PACKAGES {
            json!({})
        } else {
            json!({ "dependencies": { format!("dep-{}", index + 1): "1.0.0" } })
        };
    }
    let mut lockfile: Lockfile = serde_json::from_value(document).unwrap();
    let importer: ProjectSnapshot = serde_json::from_value(json!({
        "dependencies": { "dep-0": { "specifier": "1.0.0", "version": "1.0.0" } }
    }))
    .unwrap();
    lockfile.importers.extend((0..IMPORTERS).map(|index| {
        (format!("projects/project-{index}"), importer.clone())
    }));
    lockfile
}

pub fn bench_local_containment(criterion: &mut Criterion) {
    let lockfile = many_importers();
    let mut group = criterion.benchmark_group("lockfile_verification");
    group.throughput(Throughput::Elements(lockfile.importers.len() as u64));
    group.bench_function("overlapping_importers", |bencher| {
        bencher.iter(|| verify_lockfile_local_resolutions(black_box(&lockfile), None).unwrap());
    });
    group.finish();
}
