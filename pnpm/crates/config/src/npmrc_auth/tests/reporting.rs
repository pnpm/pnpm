use super::{Config, DeclaredRegistries, NoEnv, NpmrcAuth, Path, assert_eq};

#[test]
fn apply_registry_and_warn_drains_warnings() {
    let ini = "//reg.com/:_authToken=${MISSING}\n";
    let mut auth = NpmrcAuth::from_ini::<NoEnv>(ini, Path::new(""));
    assert_eq!(auth.warnings.len(), 1);
    let mut config = Config::new();
    auth.apply_registry_and_warn(&mut config, &mut DeclaredRegistries::default());
    assert!(auth.warnings.is_empty(), "warnings should be drained after flush");
}
