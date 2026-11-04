use super::{archive, package_json};
use pnpm_platform_variants::Platform;
use serde_json::json;

fn parse(tags: &[&str]) -> Vec<Platform> {
    tags.iter()
        .map(|tag| Platform::parse_tag(tag).unwrap())
        .collect()
}

#[test]
fn lists_the_union_of_the_variants_platform_fields() {
    let platforms = parse(&["linux-x64", "linux-arm64-musl", "darwin-arm64-macos_11_0"]);
    assert_eq!(
        package_json("example-cli", "4.0.0", &platforms),
        json!({
            "name": "example-cli",
            "version": "4.0.0",
            "os": ["darwin", "linux"],
            "cpu": ["arm64", "x64"],
            "libc": ["glibc", "musl"],
        }),
    );
    assert_eq!(
        package_json("example-cli", "4.0.0", &parse(&["win32-x64"])),
        json!({ "name": "example-cli", "version": "4.0.0", "os": ["win32"], "cpu": ["x64"] }),
    );
    assert_eq!(
        package_json("example-cli", "4.0.0", &parse(&["linux-x64", "wasm32"])),
        json!({ "name": "example-cli", "version": "4.0.0" }),
    );
}

#[test]
fn the_archive_is_the_same_whatever_order_the_platforms_come_in() {
    let first = archive("example-cli", "4.0.0", &parse(&["linux-x64", "darwin-arm64"]));
    let second = archive("example-cli", "4.0.0", &parse(&["darwin-arm64", "linux-x64"]));
    assert_eq!(first, second);
}
