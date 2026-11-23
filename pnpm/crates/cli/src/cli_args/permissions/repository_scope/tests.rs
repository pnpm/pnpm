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
