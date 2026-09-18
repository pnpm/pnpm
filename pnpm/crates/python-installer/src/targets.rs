//! The environments a project locks for: the running interpreter when it
//! declares none, and the platforms and Python versions it declares
//! otherwise.

use super::host::Interpreter;
use miette::{IntoDiagnostic, Result, bail};
use pep508_rs::MarkerTree;
use pnpm_config::Config;
use pnpm_package_is_installable::{Libc, LibcFamily, NamedPlatform, Os, SupportedArchitectures};
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
            .map(|(platform, version)| serde_json::json!({
                "platform": platform.map(interpreter_platform),
                "python": version,
            }))
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
    /// lockfile records what it was answered for. Two spellings of one
    /// platform are recorded as the one platform they name, so rewriting
    /// the configuration does not make a lockfile that still answers the
    /// project look like one that does not.
    pub(super) platforms: Vec<String>,
    pub(super) python_versions: Vec<String>,
}

impl Environments {
    pub(super) fn of(config: &Config, interpreter: &Interpreter) -> Result<Self> {
        let (platforms, python_versions) = named(config);
        let declarations = declarations(&platforms, &python_versions);
        let recorded = platforms
            .iter()
            .map(ToString::to_string)
            .collect();
        if declarations.is_empty() {
            let target = interpreter.target.clone();
            let list = vec![Environment { target, declared: Vec::new() }];
            return Ok(Self { declared: false, list, platforms: recorded, python_versions });
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
            // Two spellings of one platform, such as `linux-x64` and the
            // baseline it defaults to, are one environment, which is
            // what the interpreter reports them as.
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
        Ok(Self { declared: true, list, platforms: recorded, python_versions })
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
    platforms: &'a [NamedPlatform],
    python_versions: &'a [String],
) -> Vec<(Option<&'a NamedPlatform>, Option<&'a String>)> {
    if platforms.is_empty() && python_versions.is_empty() {
        return Vec::new();
    }
    let named = or_running(platforms.iter().map(Some).collect());
    let versions = or_running(
        python_versions
            .iter()
            .map(Some)
            .collect(),
    );
    let mut declarations = Vec::new();
    for platform in named {
        for version in &versions {
            declarations.push((platform, *version));
        }
    }
    declarations
}

/// Whether any platform this install prepares for is one pnpm can
/// resolve Python for.
pub(super) fn declares_platforms(config: &Config) -> bool {
    !named(config).0.is_empty()
}

/// The values `supportedArchitectures` names that pnpm cannot resolve
/// Python for, as the interpreter running the install reads them.
pub(super) fn platform_values_without_python(supported: &SupportedArchitectures) -> Vec<&str> {
    supported.unnamed_platform_values(
        pnpm_detect_libc::host_platform(),
        pnpm_detect_libc::host_target_arch(),
        pnpm_detect_libc::detect().map_or("unknown", |libc| libc.as_str()),
    )
}

/// The platforms and Python versions the project declares, each named
/// once.
///
/// `supportedArchitectures` says which platforms the whole install
/// prepares for, so `pylock.toml` is resolved for those. A repeat, and a
/// second spelling of one platform, is one environment.
fn named(config: &Config) -> (Vec<NamedPlatform>, Vec<String>) {
    let platforms = config.supported_architectures
        .as_ref()
        .map(SupportedArchitectures::host_platforms)
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    let python_versions = config.python.versions
        .iter()
        .filter(|version| seen.insert(*version))
        .cloned()
        .collect();
    (platforms, python_versions)
}

/// The configured values, or the one unconfigured value standing for
/// whatever the interpreter running the install reports.
fn or_running<Value>(values: Vec<Option<Value>>) -> Vec<Option<Value>> {
    if values.is_empty() { vec![None] } else { values }
}

/// How the interpreter is asked about a platform: the Rust target triple
/// of the machine, or the `manylinux` or `musllinux` baseline when the
/// platform names one, since that is what a wheel tag carries.
fn interpreter_platform(platform: &NamedPlatform) -> String {
    let architecture = platform.architecture.wheel();
    match (platform.os, &platform.libc) {
        (Os::Darwin, _) => format!("{architecture}-apple-darwin"),
        (Os::Windows, _) => format!("{architecture}-pc-windows-msvc"),
        (Os::Linux, Some(Libc { baseline: Some(baseline), .. })) => {
            format!("{architecture}-{baseline}")
        }
        (Os::Linux, libc) => {
            let family = libc.as_ref().map_or(LibcFamily::Glibc, |libc| libc.family);
            let abi = if family == LibcFamily::Musl { "musl" } else { "gnu" };
            format!("{architecture}-unknown-linux-{abi}")
        }
    }
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
         add it to supportedArchitectures and python.versions, \
         or install with an interpreter that is one of them",
        running.python_full_version(),
        running.sys_platform(),
        running.platform_machine(),
    );
}
