## 1102.1.5

### Patch Changes

- Recover from a metadata cache entry that disappears (concurrent cache cleanup, antivirus) after the registry has already answered the conditional request with `304 Not Modified`. The metadata is re-requested once without cache validators instead of failing the install with `ERR_PNPM_CACHE_MISSING_AFTER_304`.

- Fixed an out-of-memory regression when workspace projects concurrently resolve a package with large registry metadata [pnpm/pnpm#13077](https://github.com/pnpm/pnpm/issues/13077).

- Fixed `pnpm update` rewriting exact version pins that use the `=` operator (for example `=3.5.1`) to a caret range (`^3.5.1`). Exact pins are now preserved and written back as the bare version. See pnpm/pnpm#12745.
