use super::locked_archive_store_index_keys;
use std::fs;

const DIGEST: &str = "0b0f6ab1a92e94d1d9ef17bd6fb9aab2e3e0d1d6e1a9b0c4d8a9c5f2e7b1a3c4";

#[test]
fn keys_every_pinned_wheel_and_source_distribution() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pylock.toml");
    fs::write(
        &path,
        format!(
            r#"lock-version = "1.0"
created-by = "another-tool"

[[packages]]
name = "demo"
version = "1.0.0"
wheels = [{{ name = "demo-1.0.0-py3-none-any.whl", url = "https://example.test/demo.whl", size = 10, hashes = {{ sha256 = "{DIGEST}" }} }}]

[[packages]]
name = "built"
version = "2.0.0"
sdist = {{ name = "built-2.0.0.tar.gz", url = "https://example.test/built.tar.gz", hashes = {{ sha256 = "{DIGEST}" }} }}

[[packages]]
name = "local"
version = "0.1.0"
directory = {{ path = "local" }}

[[packages]]
name = "unnamed"
version = "4.0.0"
wheels = [{{ url = "https://example.test/unnamed-4.0.0-py3-none-any.whl", hashes = {{ sha256 = "{DIGEST}" }} }}]

[[packages]]
name = "undigested"
version = "3.0.0"
wheels = [{{ name = "undigested-3.0.0-py3-none-any.whl", url = "https://example.test/u.whl", hashes = {{ md5 = "00" }} }}]
"#,
        ),
    )
    .unwrap();

    let integrity = ssri::Integrity::from_hex(DIGEST, ssri::Algorithm::Sha256).unwrap();
    assert_eq!(
        locked_archive_store_index_keys(&path).unwrap(),
        Some(vec![
            format!("raw-archive\t{integrity}\tpython:demo-1.0.0-py3-none-any.whl"),
            format!("raw-archive\t{integrity}\tpython:built-2.0.0.tar.gz"),
        ]),
    );
}

#[test]
fn no_lockfile_has_no_keys() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(locked_archive_store_index_keys(&dir.path().join("pylock.toml")).unwrap(), None);
}
