use super::{Integrity, MissingIntegrityTarball, PackumentIntegrityResolver, RegistryError};
use tokio::io::AsyncReadExt;

impl PackumentIntegrityResolver<'_> {
    pub(super) async fn cached_integrity(
        &self,
        candidate: &MissingIntegrityTarball,
    ) -> Result<Option<Integrity>, RegistryError> {
        let Some((mut file, len)) = self.state.inner.storage.open_upstream_blob(
            self.namespace,
            self.name,
            &candidate.filename,
        )
        .await?
        else {
            return Ok(None);
        };
        if len > self.state.max_tarball_bytes() {
            return Ok(None);
        }
        let (integrity, checker) =
            hash_cached_file(&mut file, candidate.expected_shasum.as_ref()).await?;
        if let Some(checker) = checker
            && checker.result().is_err()
        {
            self.state.inner.storage.remove_upstream_blob(
                self.namespace,
                self.name,
                &candidate.filename,
            )
            .await?;
            return Ok(None);
        }
        Ok(Some(integrity))
    }
}

pub(in crate::server) async fn hash_cached_file(
    file: &mut tokio::fs::File,
    expected: Option<&Integrity>,
) -> std::io::Result<(Integrity, Option<ssri::IntegrityChecker>)> {
    let mut hasher = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    let mut checker = expected.map(|integrity| ssri::IntegrityChecker::new(integrity.clone()));
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.input(&buffer[..read]);
        if let Some(checker) = checker.as_mut() {
            checker.input(&buffer[..read]);
        }
    }
    Ok((hasher.result(), checker))
}
