import { nerfDart } from '@pnpm/config.registry-auth-key'
import { isError, PnpmError } from '@pnpm/error'
import normalizeRegistryUrl from 'normalize-registry-url'

/**
 * Result of parsing the structured `_auth` value.
 *
 * - `auth` — `.npmrc`-shaped URL-scoped keys (`//host/:_authToken`, ...)
 *   ready to merge into the existing auth-config pipeline.
 * - `registries` — trusted scope→URL routes inferred from the `_auth`
 *   **environment variable**. `"default"` is set by the `"@"` scope;
 *   `"@org"` by a package scope. The environment is the operator's
 *   channel — a CI runner pointed at a mandated proxy — so these outrank
 *   what any config file declares, and repo-controlled
 *   `pnpm-workspace.yaml` / project `.npmrc` cannot redirect them.
 *   Merged above workspace yaml but below CLI flags.
 * - `fallbackRegistries` — the same routes inferred from the `_auth` of
 *   the global config **file**. That file is the user's own store rather
 *   than a mandate, so a `registries` / `registry` declared in a yaml or
 *   an `.npmrc` outranks it and it only fills in what nothing else declares.
 */
export interface JsonAuthResult {
  auth: Record<string, string>
  registries: Record<string, string>
  fallbackRegistries: Record<string, string>
  defaultCandidates?: string[]
}

export function readJsonAuthEnv (env: Record<string, string | undefined>): JsonAuthResult {
  const value = readJsonAuthEnvValue(env)
  if (value == null) return { auth: {}, registries: {}, fallbackRegistries: {} }

  let parsed: unknown
  try {
    parsed = JSON.parse(value)
  } catch (err: unknown) {
    throw new PnpmError('INVALID_AUTH_SETTING', `Failed to parse pnpm_config__auth as JSON: ${isError(err) ? err.message : String(err)}`)
  }
  return parseJsonAuth(parsed, 'pnpm_config__auth')
}

/**
 * Parse a URL-keyed `_auth` object into `.npmrc`-shaped flat auth keys plus
 * the registry routes inferred from each host. Shared by the env var and the
 * global config yaml.
 *
 * Strict: any malformed entry throws. Both sources are user-controlled, so a
 * typo should fail fast rather than silently drop auth. `source` names the
 * origin in errors; raw URL keys are never echoed — they can embed secrets.
 */
function parseJsonAuth (parsed: unknown, source: string): JsonAuthResult {
  if (!isJsonObject(parsed)) {
    throw new PnpmError('INVALID_AUTH_SETTING', `${source} must be a JSON object`)
  }

  const collected: CollectedJsonAuth = { auth: {}, registries: {}, defaultCandidates: [] }
  for (const [index, [url, scopes]] of Object.entries(parsed).entries()) {
    const registry = parseJsonAuthRegistry(url, index + 1, source)
    if (!isJsonObject(scopes)) {
      throw new PnpmError('INVALID_AUTH_SETTING', `${source}[${registry.label}] must be an object keyed by scope`)
    }
    for (const [scope, rawCreds] of Object.entries(scopes)) {
      addJsonAuthScope(collected, { registry, scope, rawCreds, source })
    }
  }
  const { auth, registries, defaultCandidates } = collected
  if (defaultCandidates.length > 0) {
    registries.default = defaultCandidates[defaultCandidates.length - 1]
  }
  return { auth, registries, fallbackRegistries: {}, defaultCandidates }
}

interface CollectedJsonAuth {
  auth: Record<string, string>
  registries: Record<string, string>
  defaultCandidates: string[]
}

interface JsonAuthScopeEntry {
  registry: JsonAuthRegistry
  scope: string
  rawCreds: unknown
  source: string
}

function addJsonAuthScope (collected: CollectedJsonAuth, { registry, scope, rawCreds, source }: JsonAuthScopeEntry): void {
  if (!isJsonAuthScope(scope)) {
    throw new PnpmError('INVALID_AUTH_SETTING', `${source}[${registry.label}][${JSON.stringify(scope)}]: scope must be "@" or a package scope like "@org"`)
  }
  if (!isJsonObject(rawCreds)) {
    throw new PnpmError('INVALID_AUTH_SETTING', `${source}[${registry.label}][${JSON.stringify(scope)}] must be an auth object`)
  }
  const token = jsonAuthToken(rawCreds, registry, scope, source)
  collected.auth[`${registry.nerfed}:${scope === '@' ? '' : `${scope}:`}_authToken`] = token
  // Infer a registry route from the same entry (see JsonAuthResult.registries).
  // Last write wins on a duplicate scope, matching yaml/CLI.
  if (scope === '@') {
    collected.defaultCandidates.push(registry.normalized)
  } else {
    collected.registries[scope] = registry.normalized
  }
}

function isJsonObject (value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

/** Parse `_auth` from the global pnpm config yaml (already a parsed object). */
export function readGlobalConfigAuth (globalConfigAuth: unknown): JsonAuthResult {
  if (globalConfigAuth == null) return { auth: {}, registries: {}, fallbackRegistries: {} }
  return parseJsonAuth(globalConfigAuth, '_auth')
}

interface JsonAuthRegistry {
  label: string
  nerfed: string
  normalized: string
}

function parseJsonAuthRegistry (url: string, entryNumber: number, source: string): JsonAuthRegistry {
  const label = jsonAuthRegistryLabel(url, entryNumber)
  let parsed: URL
  try {
    parsed = new URL(url)
  } catch {
    throw new PnpmError('INVALID_AUTH_SETTING', `${source}[${label}]: key must be an http(s) registry URL`)
  }
  if ((parsed.protocol !== 'https:' && parsed.protocol !== 'http:') || parsed.hostname === '') {
    throw new PnpmError('INVALID_AUTH_SETTING', `${source}[${label}]: key must be an http(s) registry URL`)
  }
  if (parsed.username !== '' || parsed.password !== '' || parsed.search !== '' || parsed.hash !== '') {
    throw new PnpmError('INVALID_AUTH_SETTING', `${source}[${label}]: registry URL must not include credentials, query, or fragment`)
  }

  const normalized = normalizeRegistryUrl(parsed.href)
  const nerfed = nerfDart(normalized)
  if (nerfed === '') {
    throw new PnpmError('INVALID_AUTH_SETTING', `${source}[${label}]: key must be an http(s) registry URL`)
  }
  return { label, nerfed, normalized }
}

function jsonAuthRegistryLabel (url: string, entryNumber: number): string {
  const entryLabel = `entry ${entryNumber}`
  try {
    const parsed = new URL(url)
    if ((parsed.protocol === 'https:' || parsed.protocol === 'http:') && parsed.hostname !== '') {
      return `${entryLabel} (${parsed.protocol}//${parsed.host})`
    }
  } catch {}
  return entryLabel
}

function readJsonAuthEnvValue (env: Record<string, string | undefined>): string | undefined {
  return env.pnpm_config__auth !== '' && env.pnpm_config__auth != null
    ? env.pnpm_config__auth
    : env.PNPM_CONFIG__AUTH !== '' && env.PNPM_CONFIG__AUTH != null
      ? env.PNPM_CONFIG__AUTH
      : undefined
}

function isJsonAuthScope (scope: string): boolean {
  return scope === '@' || (scope.startsWith('@') && scope.length > 1 && !scope.includes('/') && !scope.includes(':'))
}

// Validate one scope's credentials and return its `authToken`. Only
// `authToken` is supported — the deprecated `basicAuth` / `username` +
// `password` forms (any other field) are rejected, as is a missing or
// non-string token.
function jsonAuthToken (
  creds: Record<string, unknown>,
  registry: JsonAuthRegistry,
  scope: string,
  source: string
): string {
  for (const field of Object.keys(creds)) {
    if (field !== 'authToken') {
      throw new PnpmError('INVALID_AUTH_SETTING', `${source}[${registry.label}][${JSON.stringify(scope)}][${JSON.stringify(field)}]: unsupported auth field (only "authToken" is supported)`)
    }
  }
  const token = creds.authToken
  if (typeof token !== 'string') {
    throw new PnpmError('INVALID_AUTH_SETTING', `${source}[${registry.label}][${JSON.stringify(scope)}]: "authToken" must be a string`)
  }
  return token
}
