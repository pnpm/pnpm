use super::{is_pnpx_alias, pnpm_exe_beside};
use pretty_assertions::assert_eq;
use std::path::PathBuf;

#[test]
fn pnpx_aliases_map_to_the_pnpm_beside_them() {
    let dir = PathBuf::from("opt").join("pnpm");
    for (exe, expected) in [
        ("pnpm.exe", "pnpm.exe"),
        ("pnpm", "pnpm"),
        ("pnpx.exe", "pnpm.exe"),
        ("PNX.EXE", "pnpm.EXE"),
        ("pnx", "pnpm"),
    ] {
        assert_eq!(pnpm_exe_beside(dir.join(exe)), dir.join(expected), "executable {exe}");
    }
}

#[test]
fn only_pnpx_and_pnx_are_aliases() {
    for stem in ["pnpx", "pnx", "PNPX", "Pnx"] {
        assert!(is_pnpx_alias(stem), "{stem} is an alias");
    }
    for stem in ["pnpm", "pn", "npx", "pnpx-extra"] {
        assert!(!is_pnpx_alias(stem), "{stem} is not an alias");
    }
}
