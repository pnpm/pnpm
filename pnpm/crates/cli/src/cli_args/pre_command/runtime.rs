use super::{
    Config, HashSet, LogEvent, PreCommandError, Value, apply_runtime_on_fail_override, global_warn,
    is_runtime_alias, sanitize_inline, system_runtime_version, version_satisfies,
};

/// pnpm's `getWantedRuntimes` + `checkRuntime`: validate every runtime the
/// root manifest pins against the runtime installed on the system.
pub(super) fn check_runtimes(
    mut manifest: Value,
    config: &Config,
    emit: fn(&LogEvent),
) -> miette::Result<()> {
    if let Some(runtime_on_fail) = config.runtime_on_fail {
        apply_runtime_on_fail_override(&mut manifest, runtime_on_fail.as_str());
    }
    // `devEngines.runtime` wins over `engines.runtime`: the first entry seen
    // for a runtime is the one that gets checked.
    let mut checked = HashSet::new();
    for engines_field in ["devEngines", "engines"] {
        for runtime in declared_runtimes(&manifest, engines_field) {
            let Some(name) = runtime.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !is_runtime_alias(name) || !checked.insert(name.to_string()) {
                continue;
            }
            check_runtime(runtime, name, emit)?;
        }
    }
    Ok(())
}

/// The runtimes one engines field declares. Both the single-object and
/// the array spellings are accepted, as pnpm does.
fn declared_runtimes<'a>(manifest: &'a Value, engines_field: &str) -> &'a [Value] {
    let Some(runtime_entry) =
        manifest.get(engines_field).and_then(|engines| engines.get("runtime"))
    else {
        return &[];
    };
    match runtime_entry {
        Value::Array(runtimes) => runtimes.as_slice(),
        runtime @ Value::Object(_) => std::slice::from_ref(runtime),
        _ => &[],
    }
}

fn check_runtime(runtime: &Value, name: &str, emit: fn(&LogEvent)) -> miette::Result<()> {
    let on_fail = runtime.get("onFail").and_then(Value::as_str);
    if matches!(on_fail, None | Some("ignore" | "download")) {
        return Ok(());
    }
    let display_name = runtime_display_name(name);
    let wanted_range = runtime.get("version").and_then(Value::as_str).map(str::trim);
    let Some(wanted_range) = wanted_range.filter(|range| !range.is_empty()) else {
        return fail_runtime_check(
            on_fail,
            &format!(
                "This project requires a {display_name} runtime but does not specify a version range",
            ),
            emit,
        );
    };
    if node_semver::Range::parse(wanted_range).is_err() {
        return fail_runtime_check(
            on_fail,
            &format!(
                "This project requires an invalid {display_name} version range: {wanted_range}",
            ),
            emit,
        );
    }
    check_installed_runtime(name, display_name, wanted_range, on_fail, emit)
}

/// Every `onFail` other than `error` — including a value pnpm does not
/// define — degrades to a warning.
fn fail_runtime_check(
    on_fail: Option<&str>,
    message: &str,
    emit: fn(&LogEvent),
) -> miette::Result<()> {
    if on_fail == Some("error") {
        let message = sanitize_inline(message).into_owned();
        return Err(PreCommandError::BadRuntimeVersion { message }.into());
    }
    global_warn(emit, message);
    Ok(())
}

fn runtime_display_name(runtime: &str) -> &'static str {
    match runtime {
        "deno" => "Deno",
        "bun" => "Bun",
        _ => "Node.js",
    }
}

pub(super) const RUNTIME_ON_FAIL_HINT: &str = r#"If you want to bypass this version check, set "runtimeOnFail" to "warn" or "ignore" (e.g. via --runtime-on-fail=ignore), or set "devEngines.runtime.onFail"/"engines.runtime.onFail" to "warn" or "ignore""#;

fn check_installed_runtime(
    name: &str,
    display_name: &str,
    wanted_range: &str,
    on_fail: Option<&str>,
    emit: fn(&LogEvent),
) -> miette::Result<()> {
    let Some(current_version) = system_runtime_version(name) else {
        return fail_runtime_check(
            on_fail,
            &format!(
                "This project requires {display_name} {wanted_range}, but {display_name} was not found on the system",
            ),
            emit,
        );
    };
    if version_satisfies(&current_version, wanted_range) {
        return Ok(());
    }
    fail_runtime_check(
        on_fail,
        &format!(
            "This project requires {display_name} {wanted_range}. Your current {display_name} is v{current_version}",
        ),
        emit,
    )
}
