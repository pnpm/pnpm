use crate::checkbox_prompt::{CheckboxAnswer, CheckboxChoice, CheckboxItem, CheckboxPrompt};
use miette::IntoDiagnostic;

pub(super) fn prompt_for_builds(names: &[String]) -> miette::Result<Option<Vec<String>>> {
    let items = super::sort_unique(names.to_vec())
        .into_iter()
        .map(|name| {
            CheckboxItem::Choice(CheckboxChoice { short: name.clone(), value: name.clone(), name })
        })
        .collect();
    let answer = CheckboxPrompt::new("Choose which packages to build", items)
        .interact()
        .into_diagnostic()?;
    Ok(match answer {
        CheckboxAnswer::Selected(names) => Some(names),
        CheckboxAnswer::Cancelled => None,
    })
}

pub(super) fn confirm_builds(names: &[String]) -> miette::Result<bool> {
    crate::confirm_prompt::confirm(
        &format!("The next packages will now be built: {}.\nDo you approve?", names.join(", ")),
        Some(false),
    )
    .into_diagnostic()
}

pub(super) fn stdin_is_terminal() -> miette::Result<bool> {
    crate::confirm_prompt::stdin_is_terminal().into_diagnostic()
}
