use std::{fs, path::Path};

const PACKAGE: &str = "injected-benchmark-library";
const FILE_COUNT: usize = 10_000;

pub(super) fn root_manifest() -> String {
    serde_json::json!({
        "name": "injected-benchmark-root",
        "private": true,
        "dependencies": { PACKAGE: "workspace:*" },
        "dependenciesMeta": { PACKAGE: { "injected": true } },
    })
    .to_string()
}

pub(super) fn create_project(root: &Path) {
    let project = root.join("packages/library");
    let data = project.join("data");
    fs::create_dir_all(&data).expect("create injected workspace package");
    fs::write(
        project.join("package.json"),
        serde_json::json!({
            "name": PACKAGE, "version": "1.0.0",
        })
        .to_string(),
    )
    .expect("write injected workspace manifest");
    for index in 0..FILE_COUNT {
        fs::write(data.join(format!("file-{index}.txt")), format!("data-{index}\n"))
            .expect("write injected workspace file");
    }
}
