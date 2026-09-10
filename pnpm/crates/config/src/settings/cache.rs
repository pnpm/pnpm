use super::{
    BTreeMap, Config, EnvVar, RemoteSideEffectsCacheSettings, side_effects_cache_remote_env,
};

impl Config {
    /// The environment is the last word on the remote side-effects cache: it is
    /// where a CI runner injects the signing material that must not be
    /// committed, and where a build job flips publication on for one
    /// invocation.
    ///
    /// Read here rather than by the installer so the values reach it as
    /// ordinary settings. A malformed JSON variable is dropped with a warning
    /// rather than failing the install, matching how the feature degrades to a
    /// local build on every other cache failure.
    pub(crate) fn apply_remote_side_effects_cache_env<Sys: EnvVar>(&mut self) {
        let mut settings = RemoteSideEffectsCacheSettings::default();
        let mut set_any = false;
        if let Some((publish, _)) = side_effects_cache_remote_env::<Sys>("PUBLISH") {
            settings.publish = Some(publish == "true");
            set_any = true;
        }
        for (field, suffix) in [
            (&mut settings.key_id, "KEY_ID"),
            (&mut settings.builder_id, "BUILDER_ID"),
            (&mut settings.image_digest, "IMAGE_DIGEST"),
            (&mut settings.architecture_baseline, "ARCHITECTURE_BASELINE"),
            (&mut settings.private_key, "PRIVATE_KEY"),
        ] {
            if let Some((value, _)) = side_effects_cache_remote_env::<Sys>(suffix) {
                *field = Some(value);
                set_any = true;
            }
        }
        for (field, suffix) in
            [(&mut settings.build_env, "BUILD_ENV"), (&mut settings.trusted_keys, "TRUSTED_KEYS")]
        {
            let Some((value, variable)) = side_effects_cache_remote_env::<Sys>(suffix) else {
                continue;
            };
            match serde_json::from_str::<BTreeMap<String, String>>(&value) {
                Ok(parsed) => {
                    *field = Some(parsed);
                    set_any = true;
                }
                Err(error) => tracing::warn!(
                    target: "pacquet::config",
                    variable,
                    %error,
                    "remote side-effects environment variable is not a string-valued JSON object",
                ),
            }
        }
        if set_any {
            self.remote_side_effects_cache.get_or_insert_default().overlay(settings);
        }
    }

    /// Apply a boolean `sideEffectsCache` declaration, which turns the
    /// local read and write gates on or off together.
    ///
    /// [`Config::side_effects_cache_read`] and
    /// [`Config::side_effects_cache_write`] prefer the object form's
    /// fields, so a layer spelling the setting as a boolean has to clear
    /// what an earlier layer's object left behind to beat it. The remote
    /// tier is a separate declaration the boolean says nothing about, so
    /// it survives untouched.
    pub fn apply_side_effects_cache_shorthand(&mut self, enabled: bool) {
        self.side_effects_cache = enabled;
        self.side_effects_cache_read_setting = None;
        self.side_effects_cache_write_setting = None;
    }

    /// Whether the install should consult the side-effects cache
    /// (`sideEffectsCacheRead = sideEffectsCache ?? sideEffectsCacheReadonly`).
    ///
    /// Pacquet collapses pnpm's tri-state (`undefined`/`true`/`false`)
    /// into two booleans: the cache is read when either flag is on, so
    /// users who only want the READ side can set
    /// `sideEffectsCacheReadonly: true` with `sideEffectsCache: false`
    /// and get a read-only view.
    pub fn side_effects_cache_read(&self) -> bool {
        self.side_effects_cache_read_setting
            .unwrap_or(self.side_effects_cache || self.side_effects_cache_readonly)
    }

    /// Whether the install is allowed to populate the side-effects
    /// cache after a successful postinstall
    /// (`sideEffectsCacheWrite = sideEffectsCache`), with the additional
    /// constraint that the explicit `sideEffectsCacheReadonly: true`
    /// always wins — a `??` would let `readonly` slip through when both
    /// flags are explicitly set, but `readonly` as a flag name only makes
    /// sense if it really does block writes.
    pub fn side_effects_cache_write(&self) -> bool {
        self.side_effects_cache_write_setting
            .unwrap_or(self.side_effects_cache && !self.side_effects_cache_readonly)
    }
}
