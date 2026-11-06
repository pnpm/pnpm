use super::{CodeSymbol, crate_of_path, fill_sizes};
use pretty_assertions::assert_eq;

fn symbol(address: u64, size: u64, name: &str) -> CodeSymbol {
    CodeSymbol { address, size, name: name.to_string() }
}

#[test]
fn crate_of_a_plain_path() {
    assert_eq!(crate_of_path("pnpm_config::Config::current"), "pnpm_config");
}

#[test]
fn crate_of_a_trait_method_is_the_crate_of_its_self_type() {
    assert_eq!(
        crate_of_path("<pnpm_lockfile::Lockfile as serde::de::Deserialize>::deserialize"),
        "pnpm_lockfile",
    );
    assert_eq!(
        crate_of_path("<&mut serde_json::de::Deserializer<R> as serde::de::Deserializer>::f"),
        "serde_json",
    );
    assert_eq!(crate_of_path("<[T] as core::fmt::Debug>::fmt"), "T");
}

#[test]
fn crate_of_a_path_without_an_identifier_is_the_path() {
    assert_eq!(crate_of_path("<()>"), "<()>");
}

#[test]
fn fill_sizes_derives_missing_sizes_from_the_next_address() {
    let symbols = vec![
        symbol(0x30, 0, "c"),
        symbol(0x10, 0, "a"),
        symbol(0x20, 4, "b"),
        symbol(0x10, 0, "a_alias"),
    ];
    assert_eq!(
        fill_sizes(symbols, 0x38),
        vec![symbol(0x10, 0x10, "a"), symbol(0x20, 4, "b"), symbol(0x30, 8, "c")],
    );
}
