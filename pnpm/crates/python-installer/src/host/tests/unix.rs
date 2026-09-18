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

fn unpacked_wheel(root: &Path, mismatched_wheel_hash: bool) -> BTreeMap<String, PathBuf> {
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
        let digest = URL_SAFE_NO_PAD.encode(digest);
        let digest = if mismatched_wheel_hash && name.ends_with("/WHEEL") {
            "A".repeat(digest.len())
        } else {
            digest
        };
        writeln!(record, "{name},sha256={digest},{}", body.len()).unwrap();
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
    let files = unpacked_wheel(&temporary.path().join("wheel"), false);
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

#[tokio::test]
async fn unpacked_wheels_tolerate_record_hash_mismatches() {
    let temporary = tempfile::tempdir().unwrap();
    let files = unpacked_wheel(&temporary.path().join("wheel"), true);
    let metadata = inspect("python3", &files).await.unwrap();
    let packages = serde_json::json!([{ "files": files, "metadata": metadata }]);
    let wheel = fs::read_to_string(files["alpha-1.0.dist-info/WHEEL"].as_path()).unwrap();
    let hash = pnpm_crypto_hash::create_hash(&wheel);
    let digest = STANDARD
        .decode(hash.strip_prefix("sha256-").unwrap())
        .unwrap();
    let expected = format!(
        "alpha-1.0.dist-info/WHEEL,sha256={},{}",
        URL_SAFE_NO_PAD.encode(digest),
        wheel.len(),
    );
    for mode in [
        PackageImportMethod::Auto,
        PackageImportMethod::CloneOrCopy,
        PackageImportMethod::Hardlink,
        PackageImportMethod::Copy,
    ] {
        let root = temporary
            .path()
            .join(format!("bad-record-{mode:?}"));
        install("python3", &root, &packages, mode).await.unwrap();
        let output = tokio::process::Command::new(root.join("bin/python"))
            .args([
                "-I",
                "-c",
                "import importlib.metadata as m; print(next(row for row in m.distribution('alpha').read_text('RECORD').splitlines() if row.startswith('alpha-1.0.dist-info/WHEEL,')))",
            ])
            .output()
            .await
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
    }
}

#[tokio::test]
async fn unpacked_wheels_reject_malformed_record_hashes_and_sizes() {
    let temporary = tempfile::tempdir().unwrap();
    for (index, (digest, size)) in [
        ("", "1"),
        ("sha1=AAAAAAAAAAAAAAAAAAAAAAAAAAA", "1"),
        ("sha256=AAAA", "1"),
        ("sha256=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "many"),
        ("sha256=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "-1"),
    ]
    .into_iter()
    .enumerate()
    {
        let files = unpacked_wheel(&temporary.path().join(index.to_string()), false);
        let record_path = files["alpha-1.0.dist-info/RECORD"].as_path();
        let record = fs::read_to_string(record_path).unwrap();
        let (first, rest) = record.split_once('\n').unwrap();
        let name = first.split_once(',').unwrap().0;
        fs::write(record_path, format!("{name},{digest},{size}\n{rest}")).unwrap();
        assert!(inspect("python3", &files).await.is_err());
    }
}
