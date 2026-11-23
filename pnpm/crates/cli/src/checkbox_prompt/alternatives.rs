use super::{
    CheckboxChoice,
    CheckboxItem,
    CheckboxPrompt,
};
use std::{
    collections::BTreeMap,
    mem,
};

pub(crate) struct CheckboxAlternative<Value> {
    group: usize,
    active: bool,
    choice: CheckboxChoice<Value>,
}

impl<Value> CheckboxAlternative<Value> {
    pub(crate) fn new(group: usize, choice: CheckboxChoice<Value>) -> Self {
        Self { group, active: false, choice }
    }
}

impl<Value> CheckboxPrompt<Value> {
    /// Pressing `r` selects the highlighted alternative and other rows in its group.
    pub(crate) fn with_alternatives(
        mut self,
        alternatives: BTreeMap<usize, CheckboxAlternative<Value>>,
    ) -> Self {
        self.selection.alternatives = alternatives;
        self
    }

    pub(super) fn toggle_alternative(&mut self) {
        let active = self.viewport.active;
        let Some(group) = self.selection.alternatives.get(&active).map(|alt| alt.group) else {
            return;
        };
        for (&index, alternative) in &mut self.selection.alternatives {
            if alternative.group == group
                && let CheckboxItem::Choice(choice) = &mut self.items[index]
            {
                mem::swap(choice, &mut alternative.choice);
                alternative.active = !alternative.active;
                self.selection.checked[index] = alternative.active || index == active;
            }
        }
    }

    pub(super) fn toggle_checked(&mut self, index: usize) {
        let checked = !self.selection.checked[index];
        self.selection.checked[index] = checked;
        if let Some(active) = self.selection.alternatives.get(&index).filter(|alt| alt.active) {
            for (&member, alternative) in &self.selection.alternatives {
                if alternative.group == active.group {
                    self.selection.checked[member] = checked;
                }
            }
        }
    }
}
