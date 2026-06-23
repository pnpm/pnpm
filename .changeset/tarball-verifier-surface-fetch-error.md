---
"pnpm": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/error": patch
---

Lockfile verification no longer reports a registry metadata fetch failure (for example a `403`/`401` on a private registry, or a network error) as `ERR_PNPM_TARBALL_URL_MISMATCH`. When the registry can't be reached to verify an entry, the install now aborts with the registry's own fetch error (such as `ERR_PNPM_FETCH_403`, which already explains the authentication situation) instead of mislabeling a transport failure as lockfile tampering. Registry fetch errors no longer leak basic-auth credentials embedded in the registry URL (`https://user:pass@host/`) into their message.
