use super::{
    Value,
    apply_runtime_on_fail_override,
};
use crate::PackageManifest;
use std::collections::{
    HashMap,
    VecDeque,
};

type PoliciesByName = HashMap<Value, VecDeque<Option<Value>>>;

#[derive(Clone)]
pub(crate) struct RuntimePolicy {
    applied: &'static str,
    declared: Vec<(&'static str, PoliciesByName)>,
}

impl PackageManifest {
    /// Apply the local runtime policy for dependency operations. Saving preserves
    /// the policies declared by the runtimes that survive dependency edits.
    pub fn apply_runtime_on_fail(&mut self, policy: &'static str) {
        if let Some(previous) = &mut self.runtime_policy {
            previous.applied = policy;
        } else {
            let declared: Vec<_> = ["devEngines", "engines"]
                .into_iter()
                .filter_map(|field| {
                    let runtime = self.value.get(field)?.get("runtime")?;
                    let policies = declared_policies(runtime);
                    (!policies.is_empty()).then_some((field, policies))
                })
                .collect();
            if declared.is_empty() {
                return;
            }
            self.runtime_policy = Some(Box::new(RuntimePolicy { applied: policy, declared }));
        }
        apply_runtime_on_fail_override(&mut self.value, policy);
    }
}

fn declared_policies(runtime: &Value) -> PoliciesByName {
    let entries = match runtime {
        Value::Array(entries) => entries.as_slice(),
        entry => std::slice::from_ref(entry),
    };
    let mut policies = PoliciesByName::new();
    for entry in entries.iter().filter_map(Value::as_object) {
        let name = entry
            .get("name")
            .cloned()
            .unwrap_or_default();
        policies
            .entry(name)
            .or_default()
            .push_back(entry.get("onFail").cloned());
    }
    policies
}

impl RuntimePolicy {
    pub(crate) fn restore(&self, effective: &Value, written: &mut Value) {
        for (field, policies) in &self.declared {
            let Some(runtime) = written
                .get_mut(*field)
                .and_then(|engines| engines.get_mut("runtime"))
            else {
                continue;
            };
            if let Some(source) = effective
                .get(*field)
                .and_then(|engines| engines.get("runtime"))
            {
                restore_policies(
                    runtime,
                    policies.clone(),
                    declared_policies(source),
                    self.applied,
                );
            }
        }
    }
}

fn restore_policies(
    runtime: &mut Value,
    mut policies: PoliciesByName,
    mut effective: PoliciesByName,
    applied: &str,
) {
    let entries = match runtime {
        Value::Array(entries) => entries.as_mut_slice(),
        entry => std::slice::from_mut(entry),
    };
    for entry in entries.iter_mut().filter_map(Value::as_object_mut) {
        let name = entry.get("name").unwrap_or(&Value::Null);
        let Some(declared) = policies.get_mut(name).and_then(VecDeque::pop_front) else { continue };
        let before_fold = effective
            .get_mut(name)
            .and_then(VecDeque::pop_front)
            .flatten();
        if before_fold.as_ref().and_then(Value::as_str) != Some(applied) {
            continue;
        }
        match declared {
            Some(policy) => {
                entry.insert("onFail".into(), policy);
            }
            None => {
                entry.remove("onFail");
            }
        }
    }
}
