---
"pnpm": patch
---

The three registry lookups are now named for what they are keyed by, so that none of them is called `registries` — a name the `registries` setting itself has taken:

| before | after |
|---|---|
| `Config.registries` | `Config.registriesByScope` |
| `Config.namedRegistries` | `Config.registriesByPrefix` |
| `Config.registryOptions` | `Config.registryOptionsByUrl` |

The same rename applies to the `RegistryContext` fields, the `Registries` and `NamedRegistries` types (now `RegistriesByScope` and `RegistriesByPrefix`), `normalizeRegistries` / `normalizeNamedRegistries` (now `normalizeRegistriesByScope` / `normalizeRegistriesByPrefix`), and the `BUILTIN_NAMED_REGISTRIES` constant (now `BUILTIN_REGISTRIES_BY_PREFIX`).

This is an internal rename: no setting, error code, or lockfile field changes. A `preResolution` hook still reads `ctx.registries`. An `updateConfig` hook is the exception, because it receives the resolved configuration. It reads and returns the per-scope and per-prefix maps as `registriesByScope` and `registriesByPrefix`. The `registries` and `namedRegistries` settings are read under the names users write them.

The pnpr resolve request sends `registriesByPrefix` where it sent `namedRegistries`. A pnpr server and its clients must be on matching versions, which is already the case for an experimental server.
