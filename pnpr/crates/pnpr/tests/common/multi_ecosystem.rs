use pnpr::{Config, Ecosystem};
use std::{fmt::Write as _, fs, path::Path};

pub fn multi_ecosystem_config(root: &Path, npm_access: &str) -> Config {
    let mut yaml = String::from("storage: ./storage\nregistries:\n");
    for ecosystem in Ecosystem::all() {
        let access = if ecosystem == Ecosystem::Npm { npm_access } else { "$all" };
        write!(yaml,
            "  {ecosystem}-internal:\n    type: hosted\n    ecosystem: {ecosystem}\n    namespace: internal\n    access: '{access}'\n  {ecosystem}-main:\n    type: router\n    sources: [{ecosystem}-internal]\n",
        ).unwrap();
    }
    yaml.push_str("defaultRegistry:\n  npm: npm-main\n  cargo: cargo-main\n  pypi: pypi-main\n  oci: oci-main\n");
    let path = root.join("config.yaml");
    fs::write(&path, yaml).unwrap();
    pnpr_fixtures::load_seeded_config(
        &path,
        "127.0.0.1:4873".parse().unwrap(),
        Some("http://pnpr.test".to_string()),
    )
}
