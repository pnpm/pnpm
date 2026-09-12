use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, LinkProbe, LoadWorkspaceYamlError,
    NpmrcAuth, Path, PathBuf, Pipe, WorkspaceSettings, read_npm_env, read_npmrc, read_npmrc_file,
    read_pnpm_env,
};

/// The merged `.npmrc` view, and the same merge restricted to sources
/// outside the repository.
pub(super) struct AuthSources {
    pub(super) npmrc_auth: NpmrcAuth,
    pub(super) trusted_auth: NpmrcAuth,
}

/// The project `.npmrc`, parsed as untrusted unless the user-level auth
/// file explicitly points at it.
fn project_auth_source<Sys>(
    project_npmrc_dir: &Path,
    user_npmrc_path: Option<&Path>,
) -> Option<NpmrcAuth>
where
    Sys: EnvVar + GetCurrentDir,
{
    let project_npmrc_path = project_npmrc_dir.join(".npmrc");
    // When npmrcAuthFile explicitly points at the project .npmrc, the user has
    // opted in to trusting it — allow auth env expansion and suppress the warning.
    // A relative value (e.g. `PNPM_CONFIG_NPMRC_AUTH_FILE=.npmrc`) is anchored
    // at the cwd — where the user-level read actually reads it from, and
    // how pnpm's `path.resolve` anchors it.
    let project_is_trusted_auth_file = user_npmrc_path.is_some_and(|user| {
        if user.is_absolute() {
            user == project_npmrc_path
        } else {
            Sys::current_dir().is_ok_and(|cwd| cwd.join(user) == project_npmrc_path)
        }
    });
    read_npmrc(project_npmrc_dir).map(|text| {
        let mut auth = if project_is_trusted_auth_file {
            NpmrcAuth::from_ini::<Sys>(&text, project_npmrc_dir)
        } else {
            NpmrcAuth::from_project_ini::<Sys>(&text, project_npmrc_dir)
        };
        auth.rescope_unscoped(&project_npmrc_path.display().to_string());
        auth
    })
}

fn auth_ini_source<Sys: EnvVar>(global_config_dir: Option<&Path>) -> Option<NpmrcAuth> {
    global_config_dir.and_then(|dir| {
        let path = dir.join("auth.ini");
        read_npmrc_file(&path).map(|text| parse_trusted_source::<Sys>(&text, dir, &path))
    })
}

fn user_auth_source<Sys>(user_npmrc_path: Option<&Path>) -> Option<NpmrcAuth>
where
    Sys: EnvVar + GetHomeDir,
{
    match user_npmrc_path {
        Some(path) => read_npmrc_file(path).map(|text| {
            // Relative `cafile`/`certfile` entries resolve against
            // the file's directory; for a bare filename (no parent)
            // that's the empty path — i.e. the process cwd — never
            // the file itself.
            let dir = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
            parse_trusted_source::<Sys>(&text, &dir, path)
        }),
        None => Sys::home_dir().and_then(|dir| {
            let path = dir.join(".npmrc");
            read_npmrc(&dir).map(|text| parse_trusted_source::<Sys>(&text, &dir, &path))
        }),
    }
}

fn parse_trusted_source<Sys: EnvVar>(text: &str, dir: &Path, path: &Path) -> NpmrcAuth {
    let mut auth = NpmrcAuth::from_ini::<Sys>(text, dir);
    auth.rescope_unscoped(&path.display().to_string());
    auth
}

/// URL-scoped credentials from `npm_config_//...` / `pnpm_config_//...`
/// environment variables. These are trusted (they come from the
/// environment, not the repository) and host-scoped by construction, so
/// they sit at the top of the precedence chain — above the project
/// `.npmrc` — following the env-over-workspace ordering.
fn env_scoped_auth_source<Sys: EnvVar>() -> Option<NpmrcAuth> {
    let auth = NpmrcAuth::from_url_scoped_env::<Sys>();
    (!auth.creds_by_scope_by_uri.is_empty()).then_some(auth)
}

/// Structured `_auth` registry auth from its two trusted sources:
/// the `pnpm_config__auth` env var and the global `config.yaml`'s
/// `_auth` key (env wins on conflict). See `from_json_sources`.
fn env_json_auth_source<Sys: EnvVar>(
    global_settings: Option<&WorkspaceSettings>,
) -> Result<Option<NpmrcAuth>, LoadWorkspaceYamlError> {
    let json_auth = global_settings
        .and_then(|settings| settings.auth.as_ref())
        .pipe(NpmrcAuth::from_json_sources::<Sys>)
        .map_err(|source| LoadWorkspaceYamlError::InvalidJsonAuth { source })?;
    let json_auth_has_content =
        !json_auth.creds_by_scope_by_uri.is_empty() || !json_auth.json_env_registries.is_empty();
    Ok(json_auth_has_content.then_some(json_auth))
}

/// Fold high-priority-first: the first present source is the base, each
/// lower source fills the gaps it left ([`NpmrcAuth::merge_under`]).
fn merge_auth_sources(sources: impl IntoIterator<Item = Option<NpmrcAuth>>) -> NpmrcAuth {
    let mut sources = sources.into_iter().flatten();
    let mut merged = sources.next().unwrap_or_default();
    for lower in sources {
        merged.merge_under(lower);
    }
    merged
}

/// Fold a source's explicitly-set settings into the running record.
///
/// Serializes `settings` to a camelCase JSON object (its `Option` fields make
/// a serialized value name exactly the keys this source set) and copies every
/// non-`null` entry into `target`, later sources overriding earlier ones. The
/// `_auth` key is dropped — it carries credentials and never belongs in
/// `pnpm config list` output (raw auth keys come from `raw_auth_config`,
/// censored at render time).
///
/// `virtualStoreType` and `enableGlobalVirtualStore` are two spellings of one
/// setting, so a source that sets either one decides both: the record follows
/// [`WorkspaceSettings::apply_to`] and fills in the spelling the source left
/// out, or `pnpm config get` would answer one of the two with the value the
/// install did not use.
/// Record what `settings` declares about registry routing, before
/// [`WorkspaceSettings::apply_to`] consumes it.
pub(super) fn note_declared_registries(
    declared: &mut crate::npmrc_auth::DeclaredRegistries,
    settings: &WorkspaceSettings,
) {
    declared.registry |= settings.registry.is_some();
    let Some(entries) = settings.registries.as_ref() else {
        return;
    };
    for scope in crate::workspace_yaml::registries::routed_scopes(entries) {
        if scope == crate::workspace_yaml::registries::DEFAULT_REGISTRY_SCOPE {
            declared.registry = true;
        } else {
            declared.scopes.insert(scope);
        }
    }
}

impl Config {
    /// The `.npmrc` layers that contribute credentials, folded into the merged
    /// view plus the trusted-only view the bootstrap and the `tokenHelper`
    /// check need.
    pub(super) fn collect_auth_sources<Sys>(
        &mut self,
        start_dir: &std::path::Path,
        workspace_yaml: Option<&(PathBuf, Option<WorkspaceSettings>)>,
        global_settings: Option<&WorkspaceSettings>,
        global_config_dir: Option<&Path>,
    ) -> Result<AuthSources, LoadWorkspaceYamlError>
    where
        Sys: EnvVar + EnvVarOs + GetCurrentDir + GetHomeDir + LinkProbe,
    {
        let user_npmrc_path = self.user_npmrc_path::<Sys>(global_settings);

        // Build the merge sources in priority order (high → low):
        // project `.npmrc` > `auth.ini` > user-level `.npmrc`. Each is
        // parsed and rescoped independently before being folded together.
        // The rescope warning names the file it read, so each source
        // labels itself with the path it was actually loaded from.
        let project_npmrc_dir =
            workspace_yaml.as_ref().map_or(start_dir, |(base_dir, _)| base_dir.as_path());
        let project_source =
            project_auth_source::<Sys>(project_npmrc_dir, user_npmrc_path.as_deref());
        let auth_ini_source = auth_ini_source::<Sys>(global_config_dir);
        let user_source = user_auth_source::<Sys>(user_npmrc_path.as_deref());
        let env_scoped_source = env_scoped_auth_source::<Sys>();
        let env_json_source = env_json_auth_source::<Sys>(global_settings)?;

        // Capture the trusted sources (everything but `project_source`) for
        // [`PackageManagerBootstrap`] before the fold below consumes them.
        let trusted_sources = [
            env_json_source.clone(),
            env_scoped_source.clone(),
            auth_ini_source.clone(),
            user_source.clone(),
        ];

        // `env_json_source` is listed before `env_scoped_source` so the JSON
        // env var wins on the rare occasion both define the same
        // `//host/:_authToken` key — the JSON auth is applied after the
        // env-scoped config, so it wins.
        let mut npmrc_auth = merge_auth_sources([
            env_json_source,
            env_scoped_source,
            project_source,
            auth_ini_source,
            user_source,
        ]);

        // Retain the merged raw `.npmrc` / `auth.ini` config keys for
        // `pnpm config get` / `pnpm config list` before the structured fields
        // are consumed below.
        self.raw_auth_config = std::mem::take(&mut npmrc_auth.raw_ini_config);

        let trusted_auth = merge_auth_sources(trusted_sources);

        // A `tokenHelper` names an executable, so it is honored only from a
        // trusted, non-repo source. Reject one that a workspace or project
        // `.npmrc` contributed by comparing the full merge against the
        // trusted-only merge before either is consumed below.
        crate::npmrc_auth::enforce_token_helper_trust(&npmrc_auth, &trusted_auth)?;

        Ok(AuthSources { npmrc_auth, trusted_auth })
    }

    /// Resolve the user-level `.npmrc` path. Precedence: the
    /// `npmrc_auth_file` field (CLI `--npmrc-auth-file` / `--userconfig`),
    /// then `PNPM_CONFIG_NPMRC_AUTH_FILE`, `PNPM_CONFIG_USERCONFIG`, the
    /// global `config.yaml`'s `npmrcAuthFile` and `npm_config_userconfig`.
    /// Each env var is empty-filtered individually (a `value !== ''` check).
    pub(super) fn user_npmrc_path<Sys: EnvVar>(
        &self,
        global_settings: Option<&WorkspaceSettings>,
    ) -> Option<PathBuf> {
        self.npmrc_auth_file.clone().or_else(|| {
            read_pnpm_env::<Sys>("npmrc_auth_file", "NPMRC_AUTH_FILE")
                .or_else(|| read_pnpm_env::<Sys>("userconfig", "USERCONFIG"))
                .map(PathBuf::from)
                .or_else(|| {
                    global_settings
                        .and_then(|settings| settings.npmrc_auth_file.clone())
                        .map(PathBuf::from)
                })
                .or_else(|| read_npm_env::<Sys>("userconfig", "USERCONFIG").map(PathBuf::from))
        })
    }
}
