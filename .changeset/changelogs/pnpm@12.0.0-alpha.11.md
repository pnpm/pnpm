## 12.0.0-alpha.11

### Patch Changes

- Fixed installs failing on Windows when a scoped dependency (`@scope/name`) had to be symlinked. Its `node_modules/@scope/name` link path was built by joining the whole alias as one segment, which left a `/` in the otherwise `\`-separated path; that forward slash reached `CreateSymbolicLinkW`, which rejects forward-slash paths with `ERROR_DIRECTORY` (os error 267). Paths are now rewritten to native separators before every filesystem call in the symlink writer.

- Fix `pnpm self-update <dist-tag>` recording the dist-tag (e.g. `next-12`) as the `packageManagerDependencies` specifier in `pnpm-lock.yaml`. It now records the resolved `devEngines.packageManager` pin, matching the manifest, so a later `--frozen-lockfile` install no longer fails with "the lockfile is not up to date".
