## 12.0.0-beta.2

### Patch Changes

- `pnpm install` no longer fails when `pnpm-lock.yaml` exists but cannot be parsed. Matching the TypeScript CLI, the install now prints an "Ignoring broken lockfile" warning, resolves dependencies from the manifests, and rewrites the lockfile. `--frozen-lockfile` still fails on a broken lockfile.

- Fresh installs no longer download the tarballs of platform-specific optional dependencies that don't match the current platform.
