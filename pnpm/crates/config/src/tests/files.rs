use super::{Config, HostNoHome, assert_eq, fs, load_with_project_and_user, tempdir, write_file};

#[test]
pub fn user_username_password_pins_to_its_own_file_registry() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    // _password is base64("pass")
    write_file(
        &user_file,
        "registry=https://trusted.example.com/\nusername=alice\n_password=cGFzcw==\n",
    );

    let config = load_with_project_and_user("registry=https://attacker.example.com/\n", user_file);

    let expected = format!("Basic {}", pnpm_network::base64_encode("alice:pass"));
    assert_eq!(
        config.auth_headers.for_url("https://trusted.example.com/pkg").as_deref(),
        Some(expected.as_str()),
    );
    assert_eq!(config.auth_headers.for_url("https://attacker.example.com/pkg"), None);
}

#[test]
pub fn gvs_default_is_off_and_paths_derive_cleanly() {
    let tmp = tempdir().unwrap();
    let config =
        Config::new().current::<HostNoHome>(tmp.path()).expect("workspace yaml absent => no error");
    assert!(!config.enable_global_virtual_store, "GVS is off by default");
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
    assert_eq!(config.global_virtual_store_dir, config.store_dir.links());
}

#[test]
pub fn yaml_global_virtual_store_dir_wins_over_derivation() {
    let tmp = tempdir().unwrap();
    let yaml_gvs = tmp.path().join("my-shared-store");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        format!("enableGlobalVirtualStore: true\nglobalVirtualStoreDir: {}\n", yaml_gvs.display()),
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.enable_global_virtual_store);
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
    assert_eq!(config.global_virtual_store_dir, yaml_gvs);
}

#[test]
pub fn virtual_store_dir_max_length_matches_pnpm_default() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    let expected = if cfg!(windows) { 60 } else { 120 };
    assert_eq!(config.virtual_store_dir_max_length, expected);
}

/// One file may carry both spellings — the canonical one wins, and the
/// pair is not a duplicate key that fails the whole parse.
#[test]
pub fn max_sockets_takes_the_canonical_spelling_when_a_file_has_both() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "maxSockets: 5\nmaxsockets: 7\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.max_sockets, Some(5));
}
