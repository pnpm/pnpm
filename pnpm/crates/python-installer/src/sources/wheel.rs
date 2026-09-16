use super::{super::registry::Registry, MAX_WHEEL_BYTES};
use miette::{IntoDiagnostic, Result, bail};
use pep508_rs::PackageName;
use pnpm_python_resolver::{Candidate, IndexCandidate, LockedWheel, WheelFilename};
use pnpm_reporter::Reporter;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use url::Url;

fn artifact_url(source: &str) -> Result<(Url, Option<String>)> {
    let pnpm_python_resolver::Source::Wheel { url, sha256 } =
        pnpm_python_resolver::Source::parse(source)?
    else {
        bail!("expected a Python wheel URL");
    };
    Ok((url, sha256))
}

impl Registry<'_> {
    pub(super) async fn fetch_url_wheel<Reporter: self::Reporter + 'static>(
        &mut self,
        name: &PackageName,
        source: &str,
    ) -> Result<()> {
        let (wheel, buffer) = self.url_wheel(source).await?;
        let filename = WheelFilename::parse(&wheel.name)?.expect("URL wheel was parsed");
        if filename.name != *name {
            bail!("Python URL requirement {name} names a wheel of {}", filename.name);
        }
        wheel.check_installable(&self.resolution.target.tags, name, &filename.version)?;
        self.resolution.packages.candidates.insert(
            name.clone(),
            BTreeMap::from([(
                filename.version.clone(),
                Candidate::Wheel(IndexCandidate { wheel, core_metadata: None }),
            )]),
        );
        let wheel =
            self.download_wheel_from_buffer::<Reporter>(name, &filename.version, buffer).await?;
        self.remember(name.clone(), filename.version, wheel);
        Ok(())
    }

    async fn url_wheel(&self, source: &str) -> Result<(LockedWheel, Option<Vec<u8>>)> {
        let (url, expected) = artifact_url(source)?;
        let name = pnpm_network::percent_decode_str(
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .unwrap_or(""),
        );
        if WheelFilename::parse(&name)?.is_none() {
            bail!("Python URL requirements currently require a wheel: {source}");
        }
        let cache = self.config.cache_dir
            .join("python-url-v1")
            .join(format!("{}.json", pnpm_crypto_hash::create_hex_hash(source)));
        if self.config.offline {
            return Ok((self.cached_url_wheel(&cache, source).await?, None));
        }
        let buffer = if expected.is_none() { Some(self.wheel_bytes(&url).await?) } else { None };
        let digest = expected.unwrap_or_else(|| {
            format!(
                "{:x}",
                Sha256::digest(buffer.as_ref().expect("hashless wheels were downloaded")),
            )
        });
        let wheel = LockedWheel {
            name,
            url: url.to_string(),
            hashes: BTreeMap::from([("sha256".to_string(), digest)]),
        };
        wheel.integrity()?;
        tokio::fs::create_dir_all(cache.parent().expect("cache has parent"))
            .await
            .into_diagnostic()?;
        pnpm_fs::write_atomic(&cache, &serde_json::to_vec(&wheel).into_diagnostic()?)
            .into_diagnostic()?;
        Ok((wheel, buffer))
    }

    async fn cached_url_wheel(&self, cache: &std::path::Path, source: &str) -> Result<LockedWheel> {
        let wheel: LockedWheel =
            super::super::cache::read_json(cache, 64 * 1024, "Python wheel cache")
                .await
                .map_err(|error| {
                    error.wrap_err(format!(
                        "Python URL wheel {source} is not cached for offline resolution",
                    ))
                })?;
        if !Candidate::Wheel(IndexCandidate { wheel: wheel.clone(), core_metadata: None })
            .matches_source(&pnpm_python_resolver::Source::parse(source)?)
        {
            bail!("cached Python wheel does not match its source");
        }
        wheel.integrity()?;
        Ok(wheel)
    }

    async fn wheel_bytes(&self, url: &Url) -> Result<Vec<u8>> {
        let response = self.client
            .get_limited_bytes_with_secure_auth_and_retry(
                url.as_str(),
                &self.index.auth,
                None,
                self.config.retry_opts(),
                MAX_WHEEL_BYTES,
            )
            .await
            .into_diagnostic()?;
        if !response.status.is_success() {
            bail!("Python wheel request returned {} for {url}", response.status);
        }
        if response.body_truncated {
            bail!("Python URL wheel exceeds {MAX_WHEEL_BYTES} bytes: {url}");
        }
        Ok(response.body)
    }
}
