use super::is_entry_path;
use std::path::Path;

#[test]
fn only_meta_and_outputs_are_entry_paths() {
    for accepted in ["meta.json", "outputs/out/result", "outputs/a"] {
        assert!(is_entry_path(Path::new(accepted)), "{accepted}");
    }
    for refused in [
        "outputs",
        "meta.json/escape",
        "../escape",
        "outputs/../../escape",
        "/escape",
        "node_modules/escape",
        "other/file",
    ] {
        assert!(!is_entry_path(Path::new(refused)), "{refused}");
    }
}
