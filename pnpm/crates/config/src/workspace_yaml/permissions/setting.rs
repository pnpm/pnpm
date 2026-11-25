use super::PackagePermissions;
use indexmap::IndexMap;
use pnpm_workspace::map_settings::{
    RuleSettings,
    Rules,
    WarningPolicy,
};
use serde::{
    Deserialize,
    Deserializer,
    de::Error,
};

type PackageRules = IndexMap<String, PackagePermissions>;

#[derive(Deserialize)]
#[serde(untagged)]
enum RuleEntry {
    Nested(PackageRules),
    Package(PackagePermissions),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum UnusedEntry {
    Policy(WarningPolicy),
    Package(PackagePermissions),
}

#[derive(Deserialize)]
struct Entries {
    rules: Option<RuleEntry>,
    unused: Option<UnusedEntry>,
    #[serde(flatten)]
    packages: PackageRules,
}

/// Read the selector map or a rules section. A package named `rules` may
/// still have its own build or skill decision in the shorthand.
pub fn deserialize_permissions<'de, Deser: Deserializer<'de>>(
    source: Deser,
) -> Result<Option<Rules<PackageRules>>, Deser::Error> {
    Option::<Entries>::deserialize(source)?.map(Entries::into_rules).transpose()
}

impl Entries {
    fn into_rules<Err: Error>(self) -> Result<Rules<PackageRules>, Err> {
        let mut packages = self.packages;
        match self.rules {
            Some(RuleEntry::Nested(rules)) => {
                if !packages.is_empty() {
                    return Err(Err::custom(
                        "put all package permissions inside permissions.rules",
                    ));
                }
                let unused = match self.unused {
                    Some(UnusedEntry::Policy(policy)) => Some(policy),
                    None => None,
                    Some(UnusedEntry::Package(_)) => {
                        return Err(Err::custom("permissions.unused must be warn or ignore"));
                    }
                };
                Ok(Rules::Section(RuleSettings { rules, unused }))
            }
            marker => {
                if let Some(RuleEntry::Package(entry)) = marker {
                    packages.insert("rules".to_string(), entry);
                }
                match self.unused {
                    Some(UnusedEntry::Package(entry)) => {
                        packages.insert("unused".to_string(), entry);
                    }
                    Some(UnusedEntry::Policy(_)) => {
                        return Err(Err::custom("permissions.unused requires permissions.rules"));
                    }
                    None => {}
                }
                Ok(Rules::Flat(packages))
            }
        }
    }
}
