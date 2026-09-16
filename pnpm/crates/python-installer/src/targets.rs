//! The environments a project locks for: the running interpreter when it
//! declares none, and the platforms and Python versions it declares
//! otherwise.

use super::host::Interpreter;
use miette::{IntoDiagnostic, Result, bail};
use pep508_rs::MarkerTree;
use pnpm_config::Config;
use pnpm_python_resolver::{Target, environment_marker};
use std::collections::BTreeSet;

/// One environment a lockfile covers: the target a resolution pass runs
/// against, and the marker variables that name it.
pub(super) struct Environment {
    pub(super) target: Target,
    /// The marker variables the declaration fixes. A declared environment
    /// names an interpreter and a platform, and its lockfile marker has
    /// to say so even when nothing in the solved graph reads them: it is
    /// what tells the environments one lockfile covers apart.
    pub(super) declared: Vec<String>,
}

/// What the interpreter is asked to report on besides itself: one entry
/// per environment the project declares, with a dimension it leaves
/// unconfigured taken from the interpreter.
pub(super) fn probe_request(config: &Config) -> serde_json::Value {
    let (platforms, python_versions) = named(config);
    serde_json::json!({
        "targets": declarations(&platforms, &python_versions)
            .into_iter()
            .map(|(platform, version)| serde_json::json!({"platform": platform, "python": version}))
            .collect::<Vec<_>>(),
    })
}

/// The environments one install locks for, in the order the lockfile
/// records them.
pub(super) struct Environments {
    /// Whether the project declares the environments it locks for. A
    /// project that declares none locks for the interpreter running the
    /// install, which is the one environment in `list`.
    pub(super) declared: bool,
    pub(super) list: Vec<Environment>,
    /// The platforms and Python versions the project declares, as the
    /// lockfile records what it was answered for. Repeats are dropped, so
    /// removing one from the configuration does not make a lockfile that
    /// still answers the project look like one that does not.
    pub(super) platforms: Vec<String>,
    pub(super) python_versions: Vec<String>,
}

impl Environments {
    pub(super) fn of(config: &Config, interpreter: &Interpreter) -> Result<Self> {
        let (platforms, python_versions) = named(config);
        let declarations = declarations(&platforms, &python_versions);
        if declarations.is_empty() {
            let target = interpreter.target.clone();
            let list = vec![Environment { target, declared: Vec::new() }];
            return Ok(Self { declared: false, list, platforms, python_versions });
        }
        if declarations.len() != interpreter.targets.len() {
            bail!(
                "the Python interpreter reported {} of the {} environments this project locks for",
                interpreter.targets.len(),
                declarations.len(),
            );
        }
        let mut list = Vec::<Environment>::new();
        for ((_, version), target) in declarations.iter().zip(&interpreter.targets) {
            let environment = Environment {
                target: target.clone(),
                declared: vec![
                    "implementation_name".to_string(),
                    "platform_machine".to_string(),
                    interpreter_version_key(version.map(String::as_str)).to_string(),
                    "sys_platform".to_string(),
                ],
            };
            // Two names for one environment, such as `linux` and the triple
            // it stands for, are what the interpreter reports them as.
            if !list
                .iter()
                .any(|kept| {
                    kept.target == environment.target && kept.declared == environment.declared
                })
            {
                list.push(environment);
            }
        }
        check_interpreter_is_declared(&list, interpreter)?;
        Ok(Self { declared: true, list, platforms, python_versions })
    }
}

/// The marker naming an environment, which is what decides whether an
/// interpreter is the one the environment stands for.
fn marker(environment: &Environment) -> Result<MarkerTree> {
    let keys = environment.declared
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    environment_marker(&environment.target.environment, &keys)?.parse().into_diagnostic()
}

/// Every declared platform paired with every declared Python version.
/// `None` is a dimension the project leaves to the interpreter running
/// the install.
fn declarations<'a>(
    platforms: &'a [String],
    python_versions: &'a [String],
) -> Vec<(Option<&'a String>, Option<&'a String>)> {
    if platforms.is_empty() && python_versions.is_empty() {
        return Vec::new();
    }
    let mut declarations = Vec::new();
    for platform in configured(platforms) {
        for version in configured(python_versions) {
            declarations.push((platform, version));
        }
    }
    declarations
}

/// The platforms and Python versions the project declares, each named
/// once. A value repeated in the configuration is one environment.
fn named(config: &Config) -> (Vec<String>, Vec<String>) {
    let once = |values: &[String]| {
        let mut named = BTreeSet::new();
        values
            .iter()
            .filter(|value| named.insert(*value))
            .cloned()
            .collect()
    };
    (once(&config.python.platforms), once(&config.python.python_versions))
}

/// The configured values, or the one unconfigured value standing for
/// whatever the interpreter running the install reports.
fn configured(values: &[String]) -> Vec<Option<&String>> {
    if values.is_empty() { vec![None] } else { values.iter().map(Some).collect() }
}

/// A declared Python version is locked for as the release it names: a
/// minor version covers its whole series, and a full one covers only
/// itself.
fn interpreter_version_key(version: Option<&str>) -> &'static str {
    match version {
        Some(version) if version.matches('.').count() == 2 => "python_full_version",
        _ => "python_version",
    }
}

/// Refuse an interpreter none of the declared environments stand for.
/// The lockfile says nothing about it: which packages it installs and
/// which wheels it takes are exactly what a declared environment answers.
fn check_interpreter_is_declared(
    environments: &[Environment],
    interpreter: &Interpreter,
) -> Result<()> {
    for environment in environments {
        if marker(environment)?.evaluate(&interpreter.target.environment, &[]) {
            return Ok(());
        }
    }
    let running = &interpreter.target.environment;
    bail!(
        "Python {} on {} {} is not one of the environments this project locks for; \
         add it to python.platforms and python.pythonVersions, \
         or install with an interpreter that is one of them",
        running.python_full_version(),
        running.sys_platform(),
        running.platform_machine(),
    );
}
