use super::{MULTI_A, MULTI_B, MULTI_C, UpdateFixture, offered, scripted_prompts};
use serde_json::json;

#[tokio::test]
async fn interactively_update_respects_negated_and_action_selectors() {
    let fixture = UpdateFixture::new();
    fixture.set_dist_tag(MULTI_A, "2.1.0", "latest");
    fixture.set_dist_tag(MULTI_C, "4.0.0", "latest");

    fixture.write_manifest(&json!({ MULTI_A: "1.0.0", MULTI_B: "2.0.0", MULTI_C: "3.0.0" }));
    fixture.update(&["update"]).await;
    fixture.write_manifest(&json!({ MULTI_A: "^1.0.0", MULTI_B: "^2.0.0", MULTI_C: "^3.0.0" }));

    let exclude_c = format!("!{MULTI_C}");
    let scripted = scripted_prompts();
    fixture.update(&[
        "update",
        "--interactive",
        "actions/checkout",
        &exclude_c,
        "--include-github-actions",
    ])
    .await;
    assert!(scripted.seen().is_empty());
    assert_eq!(
        fixture.lockfile_packages(),
        [format!("{MULTI_A}@1.0.0"), format!("{MULTI_B}@2.0.0"), format!("{MULTI_C}@3.0.0")],
    );

    scripted.answer_next(&[MULTI_A]);
    fixture.update(&["update", "--interactive", &exclude_c]).await;

    let prompts = scripted.seen();
    assert_eq!(prompts.len(), 1);
    assert_eq!(
        offered(&prompts[0]),
        [(MULTI_A.to_string(), "1.0.0".to_string(), "1.0.1".to_string())],
    );
    assert_eq!(
        fixture.lockfile_packages(),
        [format!("{MULTI_A}@1.0.1"), format!("{MULTI_B}@2.0.0"), format!("{MULTI_C}@3.0.0")],
    );
}
