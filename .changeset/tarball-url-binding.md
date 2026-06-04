---
"@pnpm/resolving.npm-resolver": minor
"@pnpm/resolving.resolver-base": minor
"@pnpm/resolving.default-resolver": patch
"@pnpm/installing.deps-installer": patch
"pnpm": minor
---

The lockfile verifier now checks that a registry entry pinning an explicit `tarball` URL points at the artifact the registry's own metadata lists for that `name@version`. Previously a tampered lockfile could pair a trusted `name@version` with an attacker-chosen tarball URL (and a matching integrity for those bytes), so the install fetched the attacker's bytes. A mismatch — or any entry that can't be confirmed against the registry — is rejected with `ERR_PNPM_TARBALL_URL_MISMATCH`. Non-registry resolutions (`file:`, git-hosted, etc.) and registry entries without an explicit tarball URL (the URL is reconstructed from name+version+registry, so it is inherently bound) are unaffected; non-standard registry tarball URLs (npm Enterprise, GitHub Packages) still pass because they match the metadata.

This binding is unconditional — it runs regardless of `minimumReleaseAge`/`trustPolicy` and is not narrowed by their exclude lists, since it guards integrity rather than maturity/trust. It is **fail-closed**: an entry passes only when the registry metadata affirmatively lists the version with a matching tarball URL. If the metadata can't be fetched, doesn't list the version, or omits `dist.tarball`, the entry is rejected. As a result, an install that re-verifies a lockfile (any install whose lockfile content changed since the last verified run, where the verification cache no longer applies) now requires the configured registry to be reachable. `trustLockfile` is the opt-out for environments that treat the on-disk lockfile as already trusted.

The `minimumReleaseAge`/`trustPolicy` verification also no longer applies to URL-keyed tarball dependencies (e.g. `https:` tarballs) that carry a semver `version` copied from their manifest — those are deliberate non-registry dependencies.
