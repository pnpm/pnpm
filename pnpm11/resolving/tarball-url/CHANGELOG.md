# @pnpm/resolving.tarball-url

## 1100.0.0

### Minor Changes

- a84d2a1: Add `@pnpm/resolving.tarball-url`, which builds and recognizes the canonical npm tarball URL of a package. It vendors `getNpmTarballUrl` (previously the external `get-npm-tarball-url` package) and adds `isCanonicalRegistryTarballUrl`, the predicate the lockfile writer uses to decide whether a tarball URL is derivable from name+version+registry (and can therefore be omitted from `pnpm-lock.yaml`).

  Exposing `isCanonicalRegistryTarballUrl` lets a custom resolver (pnpmfile `resolvers`) fronting a proxy that serves tarballs on a non-canonical path (e.g. an ephemeral `localhost:<port>`) rewrite the resolved tarball to the canonical form, so nothing host-specific is persisted to the lockfile. Previously this logic was private to `@pnpm/lockfile.utils`.

  Two correctness fixes are included while consolidating the logic: the scoped-package unescape now handles uppercase `%2F` as well as `%2f` (percent-encoding is case-insensitive), and protocol-insensitive comparison strips only a leading `http(s)://` scheme instead of splitting on the first `://` (which could truncate URLs containing a later `://`).
