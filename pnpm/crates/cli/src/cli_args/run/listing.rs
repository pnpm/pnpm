use super::{Regex, RunError, Value};
use std::fmt::Write as _;

/// The `run` positional, resolved once into whichever of pnpm's two
/// readings it is: a script name, or a `/regexp/` matching several.
///
/// Built once per command rather than per project. The recursive runner
/// applies the same selector across every selected project, so compiling
/// the pattern there would repeat the work — and would report a rejected
/// pattern once per project instead of once.
#[derive(Debug)]
pub(in super::super) struct ScriptSelector<'a> {
    pub(super) name: &'a str,
    /// `None` when the positional is a plain script name — including a
    /// regexp literal whose pattern the engine rejects, which pnpm also
    /// falls back to reading as a name.
    pub(super) pattern: Option<Regex>,
}

impl<'a> ScriptSelector<'a> {
    pub(in super::super) fn new(name: &'a str) -> Result<ScriptSelector<'a>, RunError> {
        Ok(ScriptSelector { name, pattern: try_build_regex_from_command(name)? })
    }

    /// The script names this selector picks out of `manifest`: an exact
    /// match wins, otherwise every script the pattern matches.
    pub(in super::super) fn select(&self, manifest: &Value) -> Vec<String> {
        let scripts = manifest.get("scripts").and_then(Value::as_object);
        let has_script = scripts
            .and_then(|scripts| scripts.get(self.name))
            .and_then(Value::as_str)
            .is_some_and(|script| !script.is_empty());

        if has_script {
            return vec![self.name.to_string()];
        }
        let (Some(pattern), Some(scripts)) = (self.pattern.as_ref(), scripts) else {
            return Vec::new();
        };
        scripts
            .iter()
            .filter(|(script, body)| {
                body.as_str().is_some_and(|body| !body.is_empty()) && pattern.is_match(script)
            })
            .map(|(script, _)| script.clone())
            .collect()
    }

    /// [`Self::select`] plus single-project `run`'s `start` fallback:
    /// `pnpm start` resolves to `node server.js` even when the manifest
    /// declares no `start` script. The recursive runner has no such
    /// fallback.
    pub(super) fn select_with_start(&self, manifest: &Value) -> Vec<String> {
        let specified = self.select(manifest);
        if !specified.is_empty() {
            return specified;
        }
        if self.name == "start" {
            return vec![self.name.to_string()];
        }
        Vec::new()
    }
}

/// Compile a `/pattern/` script selector, as pnpm's
/// `tryBuildRegExpFromCommand` does. `Ok(None)` means `command` is not a
/// regexp literal and addresses a script by name; a pattern the engine
/// rejects also reads as a plain name, so a mistyped selector surfaces as
/// the usual "missing script" error rather than a parser diagnostic.
fn try_build_regex_from_command(command: &str) -> Result<Option<Regex>, RunError> {
    let Some((pattern, flags)) = split_regex_literal(command) else {
        return Ok(None);
    };
    // Flags say nothing useful about which scripts to select, so pnpm
    // rejects them rather than silently honouring a subset.
    if !flags.is_empty() {
        return Err(RunError::UnsupportedScriptCommandFormat);
    }
    Ok(Regex::new(pattern).ok())
}

/// Split `/pattern/flags` into its two parts. `None` when `command` is
/// not shaped like a regexp literal: pnpm requires a non-empty pattern
/// whose only `/` characters are backslash-escaped, and flags drawn from
/// JavaScript's flag set. The closing delimiter is therefore the last
/// `/` in the string.
fn split_regex_literal(command: &str) -> Option<(&str, &str)> {
    let body = command.strip_prefix('/')?;
    let close = body.rfind('/')?;
    let (pattern, flags) = body.split_at(close);
    let flags = &flags[1..];
    if pattern.is_empty() || !flags.chars().all(|flag| "dgimuvys".contains(flag)) {
        return None;
    }
    let mut chars = pattern.chars();
    while let Some(char) = chars.next() {
        match char {
            '\\' => {
                chars.next();
            }
            '/' => return None,
            _ => {}
        }
    }
    Some((pattern, flags))
}

/// Drop hidden scripts (names starting with `.`) or reject an explicit
/// request for one.
pub(super) fn throw_or_filter_hidden_scripts(
    specified: Vec<String>,
    name: &str,
) -> Result<Vec<String>, RunError> {
    if specified.is_empty() || !specified.iter().any(|script| script.starts_with('.')) {
        return Ok(specified);
    }
    if name.starts_with('.') {
        return Err(RunError::HiddenScript { script: name.to_string() });
    }
    let visible: Vec<String> =
        specified.iter().filter(|script| !script.starts_with('.')).cloned().collect();
    if !visible.is_empty() {
        return Ok(visible);
    }
    let hidden_names =
        specified.iter().filter(|s| s.starts_with('.')).map(String::as_str).collect::<Vec<_>>();
    Err(RunError::AllHidden { scripts: hidden_names.join(", ") })
}

/// Render the script listing printed when `pnpm run` is called without a
/// script name.
pub(super) fn render_project_commands(manifest: &Value, root_manifest: Option<&Value>) -> String {
    let (lifecycle, other) = split_listed_scripts(manifest);
    if lifecycle.is_empty() && other.is_empty() {
        return "There are no scripts specified.".to_string();
    }

    let mut output = String::new();
    append_command_section(&mut output, "Lifecycle scripts:", &lifecycle);
    append_command_section(&mut output, r#"Commands available via "pnpm run":"#, &other);
    let root_scripts = root_manifest
        .and_then(|manifest| manifest.get("scripts"))
        .and_then(Value::as_object)
        .map(|scripts| {
            scripts
                .iter()
                .filter_map(|(name, script)| Some((name.as_str(), script.as_str()?)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    append_command_section(
        &mut output,
        r#"Commands of the root workspace project (to run them, use "pnpm -w run"):"#,
        &root_scripts,
    );
    output
}

/// A listing section's `(name, body)` pairs.
type ScriptListing<'a> = Vec<(&'a str, &'a str)>;

/// The project's runnable scripts, split into the lifecycle ones and the
/// rest. Hidden scripts (names starting with `.`) are not listed: they
/// can only be invoked from within another script.
fn split_listed_scripts(manifest: &Value) -> (ScriptListing<'_>, ScriptListing<'_>) {
    let mut lifecycle = Vec::new();
    let mut other = Vec::new();
    let scripts = manifest.get("scripts").and_then(Value::as_object);
    for (name, script) in scripts.into_iter().flatten() {
        if name.starts_with('.') {
            continue;
        }
        let Some(script) = script.as_str() else { continue };
        if ALL_LIFECYCLE_SCRIPTS.contains(&name.as_str()) {
            lifecycle.push((name.as_str(), script));
        } else {
            other.push((name.as_str(), script));
        }
    }
    (lifecycle, other)
}

/// Append one titled section, with a blank line between sections.
fn append_command_section(output: &mut String, title: &str, commands: &[(&str, &str)]) {
    if commands.is_empty() {
        return;
    }
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    write!(output, "{title}\n{}", render_commands(commands))
        .expect("writing to a string cannot fail");
}

fn render_commands(commands: &[(&str, &str)]) -> String {
    commands
        .iter()
        .map(|(name, script)| format!("  {name}\n    {script}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The lifecycle script names grouped separately in the run listing.
const ALL_LIFECYCLE_SCRIPTS: &[&str] = &[
    "prepublish",
    "prepare",
    "prepublishOnly",
    "prepack",
    "postpack",
    "publish",
    "postpublish",
    "preinstall",
    "install",
    "postinstall",
    "preuninstall",
    "uninstall",
    "postuninstall",
    "preversion",
    "version",
    "postversion",
    "pretest",
    "test",
    "posttest",
    "prestop",
    "stop",
    "poststop",
    "prestart",
    "start",
    "poststart",
    "prerestart",
    "restart",
    "postrestart",
    "preshrinkwrap",
    "shrinkwrap",
    "postshrinkwrap",
];
