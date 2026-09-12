use super::{Config, HostNoHome, assert_eq, fs, load_with_project_and_user, tempdir, write_file};

/// A `\n`-escaped inline PEM — the only way to fit a certificate on one
/// INI line — expands to real newlines whichever spelling declared it,
/// so rescoping an unscoped `cert`/`key` yields the same bytes as
/// writing the URL-scoped key by hand.
#[test]
pub fn unscoped_inline_pem_escapes_expand_like_the_url_scoped_spelling() {
    let cert = r"-----BEGIN CERTIFICATE-----\ncertbody\n-----END CERTIFICATE-----";
    let key = r"-----BEGIN PRIVATE KEY-----\nkeybody\n-----END PRIVATE KEY-----";
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(
        &user_file,
        &format!("registry=https://trusted.example.com/\ncert={cert}\nkey={key}\n"),
    );

    let config = load_with_project_and_user("", user_file);

    let scoped =
        config.tls_by_uri.get("//trusted.example.com/").expect("cert/key pinned to trusted");
    assert_eq!(scoped.cert.as_deref(), Some(cert.replace(r"\n", "\n").as_str()));
    assert_eq!(scoped.key.as_deref(), Some(key.replace(r"\n", "\n").as_str()));
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "preferSymlinkedExecutables is inert on Windows")]
pub fn prefer_symlinked_executables_exports_the_virtual_store_node_path() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "preferSymlinkedExecutables: true\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(
        config.extra_env.get("NODE_PATH"),
        Some(&tmp.path().join("node_modules/.pnpm/node_modules").display().to_string()),
    );
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "preferSymlinkedExecutables is inert on Windows")]
pub fn prefer_symlinked_executables_respects_an_explicit_virtual_store_dir() {
    let tmp = tempdir().unwrap();
    let virtual_store_dir = tmp.path().join("foo/bar");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        format!(
            "virtualStoreDir: {}\npreferSymlinkedExecutables: true\n",
            virtual_store_dir.display(),
        ),
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(
        config.extra_env.get("NODE_PATH"),
        Some(&virtual_store_dir.join("node_modules").display().to_string()),
    );
}
