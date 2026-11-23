use super::{
    grouped_prompt,
    package_groups,
};
use console::Key;
use pnpm_config::PermissionCapability::{
    Build,
    Skills,
};
use std::collections::BTreeMap;

#[test]
fn one_git_row_selects_none_pending_commits_or_the_repository() {
    let repo = "alpha@git+https://example.org/alpha.git";
    let first = format!("{repo}#1111111");
    let second = format!("{repo}#2222222");
    let pending = BTreeMap::from([
        (first.clone(), [Build].into()),
        (second.clone(), [Skills].into()),
        ("beta@git+https://example.org/beta.git#3333333".to_string(), [Skills].into()),
    ]);
    let mut prompt = grouped_prompt("Approve", &package_groups(&pending));
    let frame = prompt.render_frame();
    assert_eq!(frame.matches("alpha (git)").count(), 1);
    assert!(frame.contains("2 pending commits"));
    assert_eq!(prompt.selected_choices().count(), 0);
    prompt.handle_key(&Key::Char(' '));
    let selected = |choice: &crate::checkbox_prompt::CheckboxChoice<Vec<String>>| {
        choice.value.clone()
    };
    assert_eq!(
        prompt
            .selected_choices()
            .map(selected)
            .collect::<Vec<_>>(),
        vec![vec![first.clone(), second.clone()]],
    );
    prompt.handle_key(&Key::Char('r'));
    assert_eq!(
        prompt
            .selected_choices()
            .map(selected)
            .collect::<Vec<_>>(),
        vec![vec![repo.to_string()]],
    );
    prompt.handle_key(&Key::Char('r'));
    assert_eq!(
        prompt
            .selected_choices()
            .map(selected)
            .collect::<Vec<_>>(),
        vec![vec![first, second]],
    );
    prompt.handle_key(&Key::Char(' '));
    assert_eq!(prompt.selected_choices().count(), 0);
}

#[test]
fn repository_labels_redact_credentials_and_keep_the_policy_key() {
    let key =
        "demo@git+https://example-user:example-password@example.org/demo.git?token=example-query";
    let pending = BTreeMap::from([
        (format!("{key}#1234567"), [Build].into()),
        ("demo@git+https://example.org/other.git#7654321".to_string(), [Build].into()),
    ]);
    let mut prompt = grouped_prompt("Approve", &package_groups(&pending));
    for frame in [
        prompt.render_frame(),
        {
            prompt.handle_key(&Key::Char('r'));
            prompt.render_frame()
        },
        prompt.render_answer(),
    ] {
        assert!(!frame.contains("example-password"));
        assert!(!frame.contains("example-query"));
    }
    assert_eq!(
        prompt
            .selected_choices()
            .next()
            .unwrap()
            .value,
        vec![key.to_string()],
    );
}
