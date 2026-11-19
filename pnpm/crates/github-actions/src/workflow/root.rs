use super::QueryError;
use serde::de::{
    Deserialize, Deserializer, EnumAccess, IgnoredAny, MapAccess, SeqAccess, VariantAccess, Visitor,
};
use std::{fmt, marker::PhantomData};

#[derive(Default)]
pub(super) struct WorkflowRoot {
    pub(super) jobs: Vec<(String, ContainerSummary)>,
    pub(super) runs: Option<ContainerSummary>,
    pub(super) has_merge: bool,
}

#[derive(Default)]
pub(super) struct ContainerSummary {
    pub(super) uses: DirectUses,
    pub(super) steps: Option<StepField>,
    pub(super) has_merge: bool,
}

#[derive(Default)]
pub(super) enum StepField {
    #[default]
    NonSequence,
    Sequence(Vec<StepSummary>),
}

#[derive(Default)]
pub(super) struct StepSummary {
    pub(super) uses: DirectUses,
    pub(super) has_merge: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum DirectUses {
    #[default]
    Missing,
    NonString,
    StringScalar,
}

pub(super) fn parse_workflow_root(text: &str) -> Result<WorkflowRoot, QueryError> {
    yaml_serde::from_str::<WorkflowRoot>(text)
        .map_err(|err| {
            let (line, column) = match err.location() {
                Some(loc) => (loc.line(), loc.column()),
                None => (0, 0),
            };
            QueryError::InvalidInput(line, column)
        })
}

trait LooseTarget: Default {
    fn on_string(_value: String) -> Self {
        Self::default()
    }

    fn read_seq<'de, Access: SeqAccess<'de>>(mut seq: Access) -> Result<Self, Access::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Self::default())
    }

    fn read_map<'de, Access: MapAccess<'de>>(mut map: Access) -> Result<Self, Access::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Self::default())
    }
}

struct LooseVisitor<Target>(PhantomData<Target>);

impl<'de, Target: LooseTarget> Visitor<'de> for LooseVisitor<Target> {
    type Value = Target;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a YAML value")
    }

    fn visit_unit<Err>(self) -> Result<Self::Value, Err> {
        Ok(Target::default())
    }

    fn visit_none<Err>(self) -> Result<Self::Value, Err> {
        Ok(Target::default())
    }

    fn visit_some<Reader: Deserializer<'de>>(
        self,
        deserializer: Reader,
    ) -> Result<Self::Value, Reader::Error> {
        IgnoredAny::deserialize(deserializer).map(|_| Target::default())
    }

    fn visit_bool<Err>(self, _value: bool) -> Result<Self::Value, Err> {
        Ok(Target::default())
    }

    fn visit_i64<Err>(self, _value: i64) -> Result<Self::Value, Err> {
        Ok(Target::default())
    }

    fn visit_u64<Err>(self, _value: u64) -> Result<Self::Value, Err> {
        Ok(Target::default())
    }

    fn visit_f64<Err>(self, _value: f64) -> Result<Self::Value, Err> {
        Ok(Target::default())
    }

    fn visit_str<Err>(self, value: &str) -> Result<Self::Value, Err> {
        Ok(Target::on_string(value.to_string()))
    }

    fn visit_string<Err>(self, value: String) -> Result<Self::Value, Err> {
        Ok(Target::on_string(value))
    }

    fn visit_bytes<Err>(self, _value: &[u8]) -> Result<Self::Value, Err> {
        Ok(Target::default())
    }

    fn visit_seq<Access: SeqAccess<'de>>(self, seq: Access) -> Result<Self::Value, Access::Error> {
        Target::read_seq(seq)
    }

    fn visit_map<Access: MapAccess<'de>>(self, map: Access) -> Result<Self::Value, Access::Error> {
        Target::read_map(map)
    }

    fn visit_enum<Access: EnumAccess<'de>>(
        self,
        access: Access,
    ) -> Result<Self::Value, Access::Error> {
        let (_, variant) = access.variant::<IgnoredAny>()?;
        variant.newtype_variant::<IgnoredAny>()?;
        Ok(Target::default())
    }
}

macro_rules! impl_loose_deserialize {
    ($($target:ty),+ $(,)?) => {
        $(
            impl<'de> Deserialize<'de> for $target {
                fn deserialize<Reader: Deserializer<'de>>(
                    deserializer: Reader,
                ) -> Result<Self, Reader::Error> {
                    deserializer.deserialize_any(LooseVisitor(PhantomData))
                }
            }
        )+
    };
}

impl LooseTarget for WorkflowRoot {
    fn read_map<'de, Access: MapAccess<'de>>(mut map: Access) -> Result<Self, Access::Error> {
        let mut root = Self::default();
        while let Some(key) = map.next_key::<OptionalStringKey>()? {
            match key.0.as_deref() {
                Some("jobs") => root.jobs = map.next_value::<JobMap>()?.0,
                Some("runs") => root.runs = Some(map.next_value::<ContainerSummary>()?),
                Some("<<") => {
                    map.next_value::<IgnoredAny>()?;
                    root.has_merge = true;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(root)
    }
}

#[derive(Default)]
struct JobMap(Vec<(String, ContainerSummary)>);

impl LooseTarget for JobMap {
    fn read_map<'de, Access: MapAccess<'de>>(mut map: Access) -> Result<Self, Access::Error> {
        let mut jobs = Vec::new();
        while let Some(key) = map.next_key::<OptionalStringKey>()? {
            match key.0 {
                Some(name) => jobs.push((name, map.next_value::<ContainerSummary>()?)),
                None => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(Self(jobs))
    }
}

impl LooseTarget for ContainerSummary {
    fn read_map<'de, Access: MapAccess<'de>>(mut map: Access) -> Result<Self, Access::Error> {
        let mut summary = Self::default();
        while let Some(key) = map.next_key::<OptionalStringKey>()? {
            match key.0.as_deref() {
                Some("uses") => summary.uses = map.next_value::<UsesScalarProbe>()?.into_uses(),
                Some("steps") => summary.steps = Some(map.next_value::<StepField>()?),
                Some("<<") => {
                    map.next_value::<IgnoredAny>()?;
                    summary.has_merge = true;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(summary)
    }
}

impl LooseTarget for StepField {
    fn read_seq<'de, Access: SeqAccess<'de>>(mut seq: Access) -> Result<Self, Access::Error> {
        let mut steps = Vec::new();
        while let Some(step) = seq.next_element::<StepSummary>()? {
            steps.push(step);
        }
        Ok(Self::Sequence(steps))
    }
}

impl LooseTarget for StepSummary {
    fn read_map<'de, Access: MapAccess<'de>>(mut map: Access) -> Result<Self, Access::Error> {
        let mut summary = Self::default();
        while let Some(key) = map.next_key::<OptionalStringKey>()? {
            match key.0.as_deref() {
                Some("uses") => summary.uses = map.next_value::<UsesScalarProbe>()?.into_uses(),
                Some("<<") => {
                    map.next_value::<IgnoredAny>()?;
                    summary.has_merge = true;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(summary)
    }
}

#[derive(Default)]
struct UsesScalarProbe {
    is_string: bool,
}

impl UsesScalarProbe {
    fn into_uses(self) -> DirectUses {
        if self.is_string { DirectUses::StringScalar } else { DirectUses::NonString }
    }
}

impl LooseTarget for UsesScalarProbe {
    fn on_string(_value: String) -> Self {
        Self { is_string: true }
    }
}

#[derive(Default)]
struct OptionalStringKey(Option<String>);

impl LooseTarget for OptionalStringKey {
    fn on_string(value: String) -> Self {
        Self(Some(value))
    }
}

impl_loose_deserialize!(
    WorkflowRoot,
    JobMap,
    ContainerSummary,
    StepField,
    StepSummary,
    UsesScalarProbe,
    OptionalStringKey,
);
