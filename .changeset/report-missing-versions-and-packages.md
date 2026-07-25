---
"pacquet": patch
---

Resolution failures now report the error pnpm defines for them. A well-formed range that the registry publishes nothing for fails with `ERR_PNPM_NO_MATCHING_VERSION` — naming the latest release, the other dist-tags, and the `pnpm view <pkg> versions` command that lists the rest — instead of `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`. A package the registry doesn't have fails with `ERR_PNPM_FETCH_404` and the "not in the npm registry, or you have no permission to fetch it" hint (plus which authorization header was sent, since a private registry often answers a permission failure with a 404) instead of a bare HTTP-client message. A wrapper that quotes its cause verbatim no longer prints the same sentence twice in the error report.
