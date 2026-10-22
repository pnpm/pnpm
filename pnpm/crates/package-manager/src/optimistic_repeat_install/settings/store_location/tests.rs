use super::expand_home_prefix;
use pnpm_config::GetHomeDir;
use pretty_assertions::assert_eq;
use std::path::{MAIN_SEPARATOR_STR, PathBuf};

struct FakeHome;
impl GetHomeDir for FakeHome {
    fn home_dir() -> Option<PathBuf> {
        Some(PathBuf::from(MAIN_SEPARATOR_STR).join("home"))
    }
}

#[test]
fn a_leading_tilde_names_the_home_directory() {
    let expected = PathBuf::from(MAIN_SEPARATOR_STR).join("home").join("store");
    assert_eq!(expand_home_prefix::<FakeHome>("~/store"), expected.to_string_lossy());
}

#[test]
fn other_paths_are_recorded_as_configured() {
    for path in ["../store", "~store", "/abs/~/store"] {
        assert_eq!(expand_home_prefix::<FakeHome>(path), path);
    }
}
