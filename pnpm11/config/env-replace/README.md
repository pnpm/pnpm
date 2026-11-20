# @pnpm/config.env-replace

> Replaces env variable placeholders in configuration settings

## `envReplace`

Strict mode — throws when a `${VAR}` placeholder has no value and no default.

```ts
function envReplace(settingValue: string, env: NodeJS.ProcessEnv): string;
```

```ts
import { envReplace } from '@pnpm/config.env-replace'

envReplace('${foo}', process.env)
```

## `envReplaceLossy`

Lossy mode — replaces unresolved `${VAR}` placeholders with `''` and reports
them in `unresolved` instead of throwing. Resolvable placeholders and
`${VAR-default}` / `${VAR:-default}` fallbacks elsewhere in the same string
still expand normally; only the genuinely unresolved bare ones are dropped.

Use this when leaving the literal `${VAR}` in the substituted value would be
worse than dropping it (e.g. auth tokens in `.npmrc` under OIDC trusted
publishing).

```ts
function envReplaceLossy(
  settingValue: string,
  env: NodeJS.ProcessEnv
): { value: string; unresolved: string[] };
```

```ts
import { envReplaceLossy } from '@pnpm/config.env-replace'

const { value, unresolved } = envReplaceLossy('${foo}-${missing}', process.env)
// value:      'foo_value-'
// unresolved: ['${missing}']
```

## Placeholder syntax

- `${NAME}` — strict; the env var must be set (otherwise `envReplace` throws,
  or `envReplaceLossy` substitutes `''` and records the placeholder).
- `${NAME-fallback}` — `fallback` if `NAME` is unset.
- `${NAME:-fallback}` — `fallback` if `NAME` is unset **or** an empty string.
- A leading `\` escapes the placeholder (`\${NAME}` stays literal).

`{ NAME: undefined }` in `env` is treated as **unset** for both functions —
the same as the key being absent. This matches the
`Record<string, string | undefined>` shape of `NodeJS.ProcessEnv`, where
callers (notably tests) routinely model an unset variable as a
present-but-undefined property.

## License

MIT
