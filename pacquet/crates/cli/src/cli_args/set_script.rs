use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use pacquet_package_manifest::PackageManifest;
use serde_json::{Map, Value};
use std::path::Path;

/// Keys that must never be written through a property path, because they can
/// corrupt an object's prototype chain. Mirrors `UNSAFE_KEYS` in
/// `@pnpm/object.property-path`.
const UNSAFE_KEYS: [&str; 3] = ["__proto__", "constructor", "prototype"];

/// Errors from `pacquet set-script`.
///
/// Mirrors the error codes pnpm raises in `setScript.ts` and the unsafe-key
/// guard of `@pnpm/object.property-path`
/// (<https://github.com/pnpm/pnpm/blob/fc2f33912e/pnpm11/pkg-manifest/commands/src/setScript.ts>).
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum SetScriptError {
    #[display("Missing script name or command")]
    #[diagnostic(code(ERR_PNPM_SET_SCRIPT_MISSING_ARGS))]
    MissingArgs,

    #[display(r#"Key "{key}" is not allowed in a property path"#)]
    #[diagnostic(code(ERR_PNPM_UNSAFE_PROPERTY_PATH_KEY))]
    UnsafeKey { key: String },
}

/// `pacquet set-script <name> <command...>` / `pacquet ss <name> <command...>`.
///
/// Writes a `scripts` entry to the project manifest. The command is every
/// parameter after the name joined by a single space, so
/// `set-script lint eslint --fix src` stores `"eslint --fix src"`.
#[derive(Debug, Args)]
pub struct SetScriptArgs {
    /// The script name followed by the command and its arguments.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub params: Vec<String>,
}

impl SetScriptArgs {
    /// Set `scripts[name] = command` on the manifest at `manifest_path`,
    /// reading and rewriting `package.json` in place.
    ///
    /// Ports `setScript.ts`'s `handler`: a script name and a command are both
    /// required, the script name is rejected when it is a prototype-pollution
    /// key, and the name is used as a literal object key (a dot is not
    /// interpreted as a nested path).
    pub fn run(self, manifest_path: &Path) -> miette::Result<()> {
        let mut params = self.params.into_iter();
        let Some(name) = params.next() else {
            return Err(SetScriptError::MissingArgs.into());
        };
        let command_parts: Vec<String> = params.collect();
        if command_parts.is_empty() {
            return Err(SetScriptError::MissingArgs.into());
        }
        reject_unsafe_key(&name)?;
        let command = command_parts.join(" ");

        let mut manifest = PackageManifest::from_path(manifest_path.to_path_buf())
            .wrap_err("reading package.json")?;
        set_script(manifest.value_mut(), name, command);
        manifest.save().wrap_err("saving package.json")?;
        Ok(())
    }
}

/// Reject a script name that would pollute an object's prototype when used as
/// a key. Mirrors `rejectUnsafeKeys` in `@pnpm/object.property-path`.
fn reject_unsafe_key(key: &str) -> Result<(), SetScriptError> {
    if UNSAFE_KEYS.contains(&key) {
        return Err(SetScriptError::UnsafeKey { key: key.to_string() });
    }
    Ok(())
}

/// Set `scripts[name] = command` on a manifest JSON value, creating the
/// `scripts` object when it is absent and replacing it when it is present but
/// not an object. Mirrors `setObjectValueByPropertyPath(manifest, ['scripts',
/// name], command)`, which rebuilds an intermediate node whose shape disagrees
/// with the path.
fn set_script(manifest: &mut Value, name: String, command: String) {
    let Some(manifest) = manifest.as_object_mut() else { return };
    let scripts = manifest.entry("scripts").or_insert_with(|| Value::Object(Map::new()));
    if !scripts.is_object() {
        *scripts = Value::Object(Map::new());
    }
    scripts
        .as_object_mut()
        .expect("scripts is an object after the reset above")
        .insert(name, Value::String(command));
}

#[cfg(test)]
mod tests;
