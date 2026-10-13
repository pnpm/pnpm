use super::default_host_triple_in;
use pretty_assertions::assert_eq;

fn settings_file(content: &str) -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().expect("create a settings file");
    std::fs::write(file.path(), content).expect("write the settings file");
    file
}

#[test]
fn reads_the_default_host_rustup_records() {
    let file = settings_file(
        "version = \"12\"\ndefault_host_triple = \"x86_64-pc-windows-gnu\"\n\n[overrides]\n",
    );
    assert_eq!(default_host_triple_in(file.path()).as_deref(), Some("x86_64-pc-windows-gnu"));
}

#[test]
fn settings_without_a_default_host_name_none() {
    let file = settings_file("version = \"12\"\n");
    assert_eq!(default_host_triple_in(file.path()), None);
}

#[test]
fn a_missing_settings_file_names_no_default_host() {
    let dir = tempfile::tempdir().expect("create a directory");
    assert_eq!(default_host_triple_in(&dir.path().join("settings.toml")), None);
}
