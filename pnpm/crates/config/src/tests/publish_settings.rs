use super::{
    Config,
    EnvVar,
    EnvVarOs,
    GetCurrentDir,
    GetHomeDir,
    LinkProbe,
    OsString,
    Path,
    PathBuf,
    assert_eq,
    fs,
    io,
    tempdir,
};

/// `access` reads from the global `config.yaml`, is overridden by the
/// project's `pnpm-workspace.yaml`, and `PNPM_CONFIG_ACCESS` wins over both.
#[test]
pub fn access_resolves_through_every_config_layer() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(config_dir.join("config.yaml"), "access: public\n")
        .expect("write global config.yaml");
    let project = tempdir().expect("project tempdir");
    let xdg_path = xdg.path().to_str().unwrap();

    set_fake_env(&[("XDG_CONFIG_HOME", xdg_path)]);
    assert_eq!(load_with_fake_env(project.path()).access.as_deref(), Some("public"));

    fs::write(project.path().join("pnpm-workspace.yaml"), "access: restricted\n")
        .expect("write pnpm-workspace.yaml");
    assert_eq!(load_with_fake_env(project.path()).access.as_deref(), Some("restricted"));

    set_fake_env(&[("XDG_CONFIG_HOME", xdg_path), ("PNPM_CONFIG_ACCESS", "public")]);
    assert_eq!(load_with_fake_env(project.path()).access.as_deref(), Some("public"));
}

/// pnpm's `readAndFilterNpmrc` keeps only auth and network keys, so `access`
/// never comes from an `.npmrc`.
#[test]
pub fn npmrc_access_never_reaches_the_config() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join(".npmrc"), "access=restricted\n").expect("write .npmrc");
    set_fake_env(&[]);
    assert_eq!(load_with_fake_env(project.path()).access, None);
}

/// `otp` is never read from a file, so an unset placeholder in one cannot fail
/// the load the way an unset placeholder in a real setting does.
#[test]
pub fn a_workspace_otp_with_an_unset_placeholder_does_not_fail_the_load() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join("pnpm-workspace.yaml"), "otp: ${PNPM_TEST_UNSET_OTP}\n")
        .expect("write pnpm-workspace.yaml");
    set_fake_env(&[]);
    load_with_fake_env(project.path());
}
