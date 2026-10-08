use crate::checkbox_prompt::{CheckboxAnswer, CheckboxChoice, CheckboxItem, CheckboxPrompt};
use miette::IntoDiagnostic;

pub(crate) fn prompt_for_choices(
    prompt: &str,
    choices: &[(String, String)],
) -> miette::Result<Option<Vec<String>>> {
    let items = choices
        .iter()
        .map(|(value, label)| {
            CheckboxItem::Choice(CheckboxChoice {
                short: value.clone(),
                value: value.clone(),
                name: label.clone(),
            })
        })
        .collect();
    let answer = CheckboxPrompt::new(prompt, items).interact().into_diagnostic()?;
    Ok(match answer {
        CheckboxAnswer::Selected(names) => Some(names),
        CheckboxAnswer::Cancelled => None,
    })
}

pub(crate) fn confirm(message: &str) -> miette::Result<bool> {
    crate::confirm_prompt::confirm(message, Some(false)).into_diagnostic()
}

pub(super) fn stdin_is_terminal() -> miette::Result<bool> {
    crate::confirm_prompt::stdin_is_terminal().into_diagnostic()
}
