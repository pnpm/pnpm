use pnpr::{Config, Ecosystem};
use std::{fmt::Write as _, fs, path::Path};

pub fn grouped_config(root: &Path, npm_access: &str) -> Config {
    let mut yaml = String::from("storage: ./storage\nregistries:\n");
    for ecosystem in Ecosystem::all() {
        let access = if ecosystem == Ecosystem::Npm { npm_access } else { "$all" };
        write!(yaml,
            "  {ecosystem}:\n    internal:\n      type: hosted\n      access: '{access}'\n    main:\n      type: router\n      sources: [internal]\n",
        ).unwrap();
    }
    yaml.push_str("defaultRegistry:\n  npm: main\n  cargo: main\n  pypi: main\n  oci: main\n");
    let path = root.join("config.yaml");
    fs::write(&path, yaml).unwrap();
    Config::from_yaml(
        &path,
        "127.0.0.1:4873".parse().unwrap(),
        Some("http://pnpr.test".to_string()),
    )
    .unwrap()
}
