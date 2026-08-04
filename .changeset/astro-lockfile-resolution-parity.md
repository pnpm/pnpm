---
"pacquet": patch
---

Two `pnpm install` resolution fixes that made large workspaces such as [Astro](https://github.com/withastro/astro) produce a different `pnpm-lock.yaml` than pnpm 11 [#13334](https://github.com/pnpm/pnpm/issues/13334):

- A scoped workspace package referenced through the `file:` protocol (`"@test/pkg": "file:./pkg"`) is recorded as a `link:` again instead of being copied in as a `file:` snapshot.
- `bundledDependencies` / `bundleDependencies` are no longer resolved as dependencies of their own. npm ships them inside the package's tarball, so installing them again added packages the lockfile should not contain (for example `napi-wasm` under `@parcel/watcher-wasm`).
