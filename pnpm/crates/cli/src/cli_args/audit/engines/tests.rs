use super::{
    Engines,
    IncompatiblePackage,
    Origin,
    PlatformFields,
    Unsupported,
    render_incompatible,
};
use pretty_assertions::assert_eq;

fn package(name: &str, unsupported: Unsupported, via: &[&str]) -> IncompatiblePackage {
    IncompatiblePackage {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        unsupported,
        origin: Origin {
            projects: if via.is_empty() { vec![".".to_string()] } else { Vec::new() },
            via: via
                .iter()
                .map(ToString::to_string)
                .collect(),
        },
        key: format!("{name}@1.0.0").parse().expect("package key"),
    }
}

fn node(wanted: &str) -> Unsupported {
    Unsupported::Engine {
        wanted: Engines { node: Some(wanted.to_string()), pnpm: None },
        current: Engines { node: Some("26.13.0".to_string()), pnpm: None },
    }
}

fn strings(values: &[&str]) -> Option<Vec<String>> {
    Some(
        values
            .iter()
            .map(ToString::to_string)
            .collect(),
    )
}

#[test]
fn renders_what_each_package_wants_and_where_it_comes_from() {
    let fsevents = Unsupported::Platform {
        wanted: PlatformFields { os: strings(&["darwin"]), cpu: None, libc: None },
        current: PlatformFields {
            os: strings(&["linux"]),
            cpu: strings(&["x64"]),
            libc: strings(&["glibc"]),
        },
    };
    let output = render_incompatible(&[
        package("fsevents", fsevents, &["chokidar"]),
        package("gatsby-cli", node(">=18 <26"), &[]),
    ]);
    assert_eq!(
        output,
        "fsevents 1.0.0  wants os darwin, not linux x64 glibc  via chokidar\n\
         gatsby-cli 1.0.0  wants Node.js >=18 <26, not Node.js 26.13.0  .\n\n\
         2 incompatible packages",
    );
}

#[test]
fn renders_a_clean_result() {
    assert_eq!(render_incompatible(&[]), "No incompatible packages found");
}

#[test]
fn text_output_strips_control_characters_from_lockfile_labels() {
    let output = render_incompatible(&[package("evil\u{1b}[2J", node("\u{1b}[31m>=99"), &[])]);
    assert!(!output.contains('\u{1b}'), "{output:?}");
}

#[test]
fn json_names_the_unsupported_kind_and_omits_unset_fields() {
    let json = serde_json::to_value(package("old", node("0.10"), &["parent"])).expect("JSON");
    assert_eq!(
        json,
        serde_json::json!({
            "name": "old",
            "version": "1.0.0",
            "unsupported": "engine",
            "wanted": { "node": "0.10" },
            "current": { "node": "26.13.0" },
            "via": ["parent"],
        }),
    );
}
