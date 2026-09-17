use super::super::{inspect, install};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use pnpm_config::PackageImportMethod;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

fn unpacked_wheel(root: &Path) -> BTreeMap<String, PathBuf> {
    let mut files = BTreeMap::new();
    let mut record = String::new();
    for (name, body) in [
        ("alpha-1.0.dist-info/METADATA", "Metadata-Version: 2.4\nName: alpha\nVersion: 1.0\n"),
        (
            "alpha-1.0.dist-info/WHEEL",
            "Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
        ),
        ("alpha-1.0.data/scripts/native", "#!/bin/sh\nprintf native\n"),
        ("alpha/plain-exec", "private data\n"),
    ] {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        fs::set_permissions(
            &path,
            fs::Permissions::from_mode(if name.ends_with("/native") { 0o755 } else { 0o644 }),
        )
        .unwrap();
        let hash = pnpm_crypto_hash::create_hash(body);
        let digest = STANDARD
            .decode(hash.strip_prefix("sha256-").unwrap())
            .unwrap();
        writeln!(record, "{name},sha256={},{}", URL_SAFE_NO_PAD.encode(digest), body.len()).unwrap(
        );
        files.insert(name.to_string(), path);
    }
    let name = "alpha-1.0.dist-info/RECORD";
    writeln!(record, "{name},,").unwrap();
    let path = root.join(name);
    fs::write(&path, record).unwrap();
    files.insert(name.to_string(), path);
    files
}

#[tokio::test]
async fn unpacked_executables_keep_permissions_without_cas_names() {
    let temporary = tempfile::tempdir().unwrap();
    let files = unpacked_wheel(&temporary.path().join("wheel"));
    let metadata = inspect("python3", &files).await.unwrap();
    let packages = serde_json::json!([{ "files": files, "metadata": metadata }]);
    for mode in [
        PackageImportMethod::Auto,
        PackageImportMethod::CloneOrCopy,
        PackageImportMethod::Hardlink,
        PackageImportMethod::Copy,
    ] {
        let root = temporary
            .path()
            .join(format!("{mode:?}"));
        install("python3", &root, &packages, mode).await.unwrap();
        let output = tokio::process::Command::new(root.join("bin/native")).output().await.unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"native");
        let output = tokio::process::Command::new(root.join("bin/python"))
            .args(["-I", "-c", "import os, sysconfig; from pathlib import Path; p = Path(sysconfig.get_path('purelib')) / 'alpha/plain-exec'; assert not os.access(p, os.X_OK)"])
            .output().await.unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }
}
