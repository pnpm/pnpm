use super::{MULTI_A, UpdateFixture, offered, scripted_prompts};
use serde_json::json;

#[tokio::test]
async fn interactively_update_latest_preserves_named_registry() {
    let fixture = UpdateFixture::with_config(|config| {
        config.registries_by_prefix.insert("work".to_string(), config.registry.clone());
        config.minimum_release_age = Some(10_000_000);
        config.minimum_release_age_exclude = Some(vec!["@pnpm.e2e/*".to_string()]);
    });
    fixture.set_dist_tag(MULTI_A, "2.1.0", "latest");
    fixture.write_manifest(&json!({ MULTI_A: "work:1.0.0" }));
    fixture.update(&["update"]).await;

    let scripted = scripted_prompts();
    scripted.answer_next(&[MULTI_A]);
    fixture.update(&["update", "--interactive", "--latest"]).await;

    let prompts = scripted.seen();
    assert_eq!(prompts.len(), 1);
    assert_eq!(
        offered(&prompts[0]),
        [(MULTI_A.to_string(), "1.0.0".to_string(), "2.1.0".to_string())],
    );
    assert_eq!(fixture.lockfile_packages(), [format!("{MULTI_A}@work:2.1.0")]);
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.project.join("package.json")).expect("read manifest"),
    )
    .expect("parse manifest");
    assert_eq!(manifest["dependencies"][MULTI_A], "work:2.1.0");
}
