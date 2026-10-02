use crate::checkbox_prompt::{CheckboxAnswer, CheckboxChoice, CheckboxItem, CheckboxPrompt};
use dialoguer::{Error, Result};
use std::io;

#[derive(Default)]
pub(crate) struct MultiSelect {
    message: String,
    names: Vec<String>,
    defaults: Vec<bool>,
}

impl MultiSelect {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_prompt(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    pub(crate) fn items<Label: ToString>(mut self, items: &[Label]) -> Self {
        self.names = items
            .iter()
            .map(ToString::to_string)
            .collect();
        self.defaults = vec![false; items.len()];
        self
    }

    pub(crate) fn items_checked<Label: ToString>(
        mut self,
        items: impl IntoIterator<Item = (Label, bool)>,
    ) -> Self {
        (self.names, self.defaults) = items
            .into_iter()
            .map(|(name, checked)| (name.to_string(), checked))
            .unzip();
        self
    }

    pub(crate) fn interact_opt(self) -> Result<Option<Vec<usize>>> {
        let prompt = self.prompt();
        answer(prompt)
    }

    pub(crate) fn interact(self) -> Result<Vec<usize>> {
        self.interact_opt()?.ok_or_else(cancelled)
    }

    fn prompt(self) -> CheckboxPrompt<usize> {
        let items = self.names
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                CheckboxItem::Choice(CheckboxChoice { short: name.clone(), name, value: index })
            })
            .collect();
        CheckboxPrompt::new(self.message, items).dialoguer_defaults(self.defaults)
    }
}

#[derive(Default)]
pub(crate) struct Select {
    choices: MultiSelect,
    default: usize,
}

impl Select {
    pub(crate) fn new() -> Self {
        <Self as Default>::default()
    }

    pub(crate) fn with_prompt(mut self, message: impl Into<String>) -> Self {
        self.choices = self.choices.with_prompt(message);
        self
    }

    pub(crate) fn items<Label: ToString>(mut self, items: &[Label]) -> Self {
        self.choices = self.choices.items(items);
        self
    }

    pub(crate) fn default(mut self, default: usize) -> Self {
        self.default = default;
        self
    }

    pub(crate) fn interact(self) -> Result<usize> {
        if self.default >= self.choices.names.len() {
            return Err(Error::IO(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid selection default",
            )));
        }
        let selected =
            answer(self.choices.prompt().single_selection(self.default))?.ok_or_else(cancelled)?;
        selected
            .into_iter()
            .next()
            .ok_or_else(|| Error::IO(io::Error::other("Selection returned no item")))
    }
}

#[derive(Default)]
pub(crate) struct Input {
    message: String,
}

impl Input {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_prompt(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    pub(crate) fn interact_text(self) -> Result<String> {
        pnpm_wasm_host::input(&self.message, false).map_err(Error::IO)
    }
}

fn answer(prompt: CheckboxPrompt<usize>) -> Result<Option<Vec<usize>>> {
    match prompt.interact().map_err(Error::IO)? {
        CheckboxAnswer::Selected(values) => Ok(Some(values)),
        CheckboxAnswer::Cancelled => Ok(None),
    }
}

fn cancelled() -> Error {
    Error::IO(io::Error::new(io::ErrorKind::Interrupted, "Terminal selection cancelled"))
}
