use super::{
    Delegation, INCLUDE_KEY, Include, IncludeError, IncludeSource, SettingSlot,
    selector::defined_slots,
};
use std::collections::BTreeMap;
use yaml_serde::Mapping;

/// Which source owns each setting of a workspace.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ownership {
    /// Slots a setting list reserves, by the include that lists them.
    reserved: BTreeMap<SettingSlot, usize>,
    /// Slots a file defines without a reservation.
    defined: BTreeMap<SettingSlot, Claimant>,
}

/// The source that owns a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingOwner<'a> {
    /// No source delegates or defines the setting.
    Unowned,
    /// The root manifest defines the setting.
    Root,
    /// An included source owns the setting.
    Include(&'a IncludeSource),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Claimant {
    Root,
    Include(usize),
}

impl Ownership {
    /// Check every reservation and definition and record its owner.
    ///
    /// `fragments` pairs each local source's settings with its index in
    /// `includes`.
    pub(super) fn build(
        root: &Mapping,
        includes: &[Include],
        fragments: &[(usize, Mapping)],
    ) -> Result<Self, IncludeError> {
        let mut ownership = Ownership::default();
        for (position, include) in includes.iter().enumerate() {
            if let Delegation::Listed(slots) = &include.delegation {
                ownership.reserve(position, slots, includes)?;
            }
        }
        ownership.define(Claimant::Root, root, includes)?;
        for (position, fragment) in fragments {
            ownership.define(Claimant::Include(*position), fragment, includes)?;
        }
        Ok(ownership)
    }

    /// Every slot a source reserves or defines.
    pub fn slots(&self) -> impl Iterator<Item = &SettingSlot> {
        self.reserved.keys().chain(self.defined.keys())
    }

    /// The source that owns `slot`.
    #[must_use]
    pub fn owner<'a>(&self, slot: &SettingSlot, includes: &'a [Include]) -> SettingOwner<'a> {
        let claimant = self
            .reservation_of(slot)
            .map(Claimant::Include)
            .or_else(|| self.defined.get(slot).copied());
        match claimant {
            None => SettingOwner::Unowned,
            Some(Claimant::Root) => SettingOwner::Root,
            Some(Claimant::Include(position)) => SettingOwner::Include(&includes[position].source),
        }
    }

    /// The include that reserves `slot`, directly or through a reservation of
    /// every catalog.
    fn reservation_of(&self, slot: &SettingSlot) -> Option<usize> {
        self.reserved
            .get(slot)
            .copied()
            .or_else(|| match slot {
                SettingSlot::Catalog(_) => self.reserved.get(&SettingSlot::AllCatalogs).copied(),
                _ => None,
            })
    }

    fn reserve(
        &mut self,
        position: usize,
        slots: &[SettingSlot],
        includes: &[Include],
    ) -> Result<(), IncludeError> {
        for slot in slots {
            let overlapping = self.reserved
                .iter()
                .find(|(reserved, _)| reserved.covers(slot) || slot.covers(reserved));
            if let Some((reserved, &other)) = overlapping {
                return Err(IncludeError::DuplicateSetting {
                    setting: reserved.min(slot).to_string(),
                    first: format!("delegated to {}", includes[other].source),
                    second: format!("delegated to {}", includes[position].source),
                });
            }
            self.reserved.insert(slot.clone(), position);
        }
        Ok(())
    }

    fn define(
        &mut self,
        claimant: Claimant,
        settings: &Mapping,
        includes: &[Include],
    ) -> Result<(), IncludeError> {
        for (key, value) in settings {
            let Some(key) = key.as_str() else {
                continue;
            };
            if key == INCLUDE_KEY {
                continue;
            }
            for slot in defined_slots(key, value) {
                self.define_slot(claimant, slot, includes)?;
            }
        }
        Ok(())
    }

    fn define_slot(
        &mut self,
        claimant: Claimant,
        slot: SettingSlot,
        includes: &[Include],
    ) -> Result<(), IncludeError> {
        let describe = |claimant| describe_claimant(claimant, includes);
        if let Claimant::Include(position) = claimant {
            check_permitted(&includes[position], &slot)?;
        }
        if let Some(owner) = self
            .reservation_of(&slot)
            .filter(|&owner| claimant != Claimant::Include(owner))
        {
            return Err(IncludeError::DuplicateSetting {
                setting: slot.to_string(),
                first: format!("delegated to {}", describe(Claimant::Include(owner))),
                second: format!("defined in {}", describe(claimant)),
            });
        }
        if let Some(&other) = self.defined
            .get(&slot)
            .filter(|&&other| other != claimant)
        {
            return Err(IncludeError::DuplicateSetting {
                setting: slot.to_string(),
                first: format!("defined in {}", describe(other)),
                second: format!("defined in {}", describe(claimant)),
            });
        }
        if self.reservation_of(&slot).is_none() {
            self.defined.insert(slot, claimant);
        }
        Ok(())
    }
}

/// A listed source may define only the slots its list covers.
fn check_permitted(include: &Include, slot: &SettingSlot) -> Result<(), IncludeError> {
    match &include.delegation {
        Delegation::Listed(slots)
            if !slots
                .iter()
                .any(|listed| listed.covers(slot)) =>
        {
            Err(IncludeError::SettingNotDelegated {
                setting: slot.to_string(),
                spec: include.source.to_string(),
            })
        }
        _ => Ok(()),
    }
}

fn describe_claimant(claimant: Claimant, includes: &[Include]) -> String {
    match claimant {
        Claimant::Root => crate::WORKSPACE_MANIFEST_FILENAME.to_string(),
        Claimant::Include(position) => includes[position].source.to_string(),
    }
}
