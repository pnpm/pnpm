import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { envReplaceLossy } from '@pnpm/config.env-replace'
import { nerfDart } from '@pnpm/config.nerf-dart'
import normalizeRegistryUrl from 'normalize-registry-url'
import { readIniFileSync } from 'read-ini-file'

import { isNpmrcReadableKey } from './localConfig.js'
import { npmDefaults } from './npmDefaults.js'

export interface NpmrcConfigResult {
  /**
   * Merged auth/registry config from all sources.
   * Priority (lowest to highest): builtin < defaults < user < auth.ini < workspace < env (//-scoped) < CLI
   */
  mergedConfig: Record<string, unknown>
  /** Raw config suitable for pnpmConfig.authConfig (filtered through pickIniConfig by consumer) */
  rawConfig: Record<string, unknown>
  /** Non-project npmrc config used for package-manager bootstrap */
  trustedConfig: Record<string, unknown>
  /** Workspace .npmrc data */
  workspaceNpmrc: Record<string, unknown>
  /** User ~/.npmrc data (for token helpers) */
  userConfig: Record<string, unknown>
  /** Resolved local prefix (CWD or nearest dir with package.json) */
  localPrefix: string
  /** Warnings generated during loading */
  warnings: string[]
}

export interface LoadNpmrcConfigOpts {
  cliOptions: Record<string, unknown>
  defaultOptions: Record<string, unknown>
  /** Explicit working directory (from --dir flag) */
  dir?: string
  /** Workspace directory */
  workspaceDir?: string
  /** Custom path to user .npmrc (from npmrcAuthFile setting, overrides ~/.npmrc) */
  npmrcAuthFile?: string
  /** pnpm config directory (for pnpm auth file) */
  configDir: string
  /** Module directory for pnpm builtin rc */
  moduleDirname: string
  env?: Record<string, string | undefined>
}

interface ReadAndFilterNpmrcOptions {
  expandAuthValueEnv?: boolean
  expandRequestDestinationEnv?: boolean
}

export function loadNpmrcConfig (opts: LoadNpmrcConfigOpts): NpmrcConfigResult {
  const warnings: string[] = []
  const env = opts.env ?? process.env as Record<string, string | undefined>

  const localPrefix = opts.dir
    ? path.resolve(opts.dir)
    : findLocalPrefix(process.cwd())

  const userConfigPath = normalizePath(opts.npmrcAuthFile) ?? path.resolve(os.homedir(), '.npmrc')

  // Read .npmrc from workspace root (or project root if no workspace)
  const workspaceNpmrcDir = opts.workspaceDir ?? localPrefix
  const workspaceNpmrc = readAndFilterNpmrc(
    path.resolve(workspaceNpmrcDir, '.npmrc'),
    warnings,
    env,
    { expandAuthValueEnv: false, expandRequestDestinationEnv: false }
  )

  // Read user .npmrc (from npmrcAuthFile setting or ~/.npmrc)
  const userConfig = readAndFilterNpmrc(userConfigPath, warnings, env)

  // Read pnpm auth file (~/.config/pnpm/auth.ini)
  const pnpmAuthConfig = readAndFilterNpmrc(
    path.join(opts.configDir, 'auth.ini'),
    warnings,
    env
  )

  // Apply the same per-source rescope to CLI options so an unscoped
  // `--_authToken` follows the same trust rule as one written into an .npmrc.
  // We clone first to avoid mutating the caller's cliOptions object.
  const cliOptions = rescopeUnscopedCreds({ ...opts.cliOptions }, '<command line>', warnings)

  // URL-scoped auth/registry settings supplied via `npm_config_//…` and
  // `pnpm_config_//…` environment variables. The registry a credential is
  // bound to is encoded in the (trusted) variable name, so unlike a project
  // `.npmrc` these cannot be redirected to another host by the repository —
  // making them a safe, file-free way to configure registry authentication.
  const envScopedConfig = readUrlScopedEnvConfig(env)

  // Read pnpm builtin rc + inline defaults
  const pnpmBuiltinConfig: Record<string, unknown> = {
    ...readAndFilterNpmrc(
      path.resolve(path.join(opts.moduleDirname, 'pnpmrc')),
      warnings,
      env
    ),
    registry: 'https://registry.npmjs.org/',
    '@jsr:registry': 'https://npm.jsr.io/',
  }

  // Handle cafile: expand to ca certs.
  // Priority: CLI > workspace > auth.ini > user > defaults
  loadCAFile([
    cliOptions,
    workspaceNpmrc,
    pnpmAuthConfig,
    userConfig,
    opts.defaultOptions,
  ])

  // Merge all sources (lowest to highest priority):
  // builtin < defaults < user < auth.ini < workspace < env (//-scoped) < CLI
  const mergedConfig: Record<string, unknown> = {}
  for (const source of [pnpmBuiltinConfig, opts.defaultOptions, userConfig, pnpmAuthConfig, workspaceNpmrc, envScopedConfig, cliOptions]) {
    for (const [key, value] of Object.entries(source)) {
      if (isNpmrcReadableKey(key)) {
        mergedConfig[key] = value
      }
    }
  }

  // The env-scoped config is trusted (it comes from the environment, not the
  // repository), so it is included here while the workspace .npmrc is not.
  const trustedConfig: Record<string, unknown> = {}
  for (const source of [pnpmBuiltinConfig, opts.defaultOptions, userConfig, pnpmAuthConfig, envScopedConfig, cliOptions]) {
    for (const [key, value] of Object.entries(source)) {
      if (isNpmrcReadableKey(key)) {
        trustedConfig[key] = value
      }
    }
  }

  // Build rawConfig with same priority order
  const rawConfig = {
    ...pnpmBuiltinConfig,
    ...opts.defaultOptions,
    ...userConfig,
    ...pnpmAuthConfig,
    ...workspaceNpmrc,
    ...envScopedConfig,
    ...cliOptions,
  }

  return {
    mergedConfig,
    rawConfig,
    trustedConfig,
    workspaceNpmrc,
    userConfig,
    localPrefix,
    warnings,
  }
}

// Matches `npm_config_//…` and `pnpm_config_//…` env var names. The prefix is
// matched case-insensitively (as npm does), but the captured key keeps its
// original case because URL-scoped keys are case-sensitive (e.g. `:_authToken`).
const URL_SCOPED_ENV_RE = /^p?npm_config_(\/\/.+)$/i

// Collect URL-scoped settings (keys beginning with `//host…`, such as
// `//registry.npmjs.org/:_authToken`) from `npm_config_//…` and `pnpm_config_//…`
// environment variables. These are host-scoped by construction — the registry
// the value applies to is part of the variable name — so they are safe to honor
// from the trusted environment without a config file. When the same key is set
// through both prefixes, `pnpm_config_` wins.
//
// An empty value is treated as unset, matching how pnpm reads its other env
// config (`readEnvVar`'s `!== ''` filter) and npm's own `npm_config_*` handling.
function readUrlScopedEnvConfig (env: Record<string, string | undefined>): Record<string, unknown> {
  const npmScoped: Record<string, string> = {}
  const pnpmScoped: Record<string, string> = {}
  for (const envKey of Object.keys(env)) {
    const value = env[envKey]
    if (value == null || value === '') continue
    const match = URL_SCOPED_ENV_RE.exec(envKey)
    if (match == null) continue
    const key = match[1]
    // `tokenHelper` names an executable pnpm runs. It is only allowed from a
    // user-level config file (enforced by the TOKEN_HELPER_IN_PROJECT_CONFIG
    // check in index.ts, which validates against the user `.npmrc`). The env
    // layer isn't that file, so honoring `//host/:tokenHelper` here would
    // trip that guard — never admit it.
    if (key.endsWith(':tokenHelper')) continue
    const target = envKey.slice(0, 5).toLowerCase() === 'pnpm_' ? pnpmScoped : npmScoped
    target[key] = value
  }
  return { ...npmScoped, ...pnpmScoped }
}

// Per-registry rc keys that, when written without a `//host/` prefix, fall
// through to whatever default registry the merged config settles on. We
// rewrite each such key to its URL-scoped form at load time, pinning it to
// the `registry=` value declared in the same source. A later layer can
// still override the merged registry, but it cannot pull along a credential
// or client certificate authored for a different host.
//
// Two groups:
// * auth keys — `_authToken` etc. Pinned to prevent credential leaks. npm
//   rejects these unscoped since npm@9 (ERR_INVALID_AUTH); pnpm keeps them
//   working but warns so users migrate before a future major drops support.
// * client certificate keys — `cert`/`key` (inline PEM). Pinned to prevent
//   a client certificate (and the identity it carries) being presented to
//   the wrong host. The `certfile`/`keyfile` path variants are not in
//   `NPM_AUTH_SETTINGS`, so unscoped forms never reach the merged config
//   in the first place — only the URL-scoped `//host/:certfile=...` and
//   `//host/:keyfile=...` forms are honored, and those are already pinned
//   to their authoring registry by construction.
//
// `ca`/`cafile` are intentionally left unscoped-by-default: they're trust
// anchors, not credentials, and corporate MITM-proxy setups rely on them
// applying globally to every HTTPS request. The default registry override
// can't weaponize an unscoped CA (the attacker would need a cert signed
// by it), so the same pinning isn't warranted.
const UNSCOPED_RESCOPABLE_KEYS = [
  '_authToken', '_auth', 'username', '_password', 'tokenHelper',
  'cert', 'key',
] as const

function readAndFilterNpmrc (
  filePath: string,
  warnings: string[],
  env: Record<string, string | undefined>,
  opts: ReadAndFilterNpmrcOptions = {}
): Record<string, unknown> {
  let raw: Record<string, unknown>
  try {
    raw = readIniFileSync(filePath) as Record<string, unknown>
  } catch (err: unknown) {
    if (isErrorWithCode(err, 'ENOENT') || isErrorWithCode(err, 'EISDIR')) {
      return {}
    }
    warnings.push(`Issue while reading "${filePath}". ${err instanceof Error ? err.message : String(err)}`)
    return {}
  }

  const npmrcDir = path.dirname(filePath)
  const result: Record<string, unknown> = {}
  const expandAuthValueEnv = opts.expandAuthValueEnv ?? true
  const expandRequestDestinationEnv = opts.expandRequestDestinationEnv ?? true
  for (const [rawKey, rawValue] of Object.entries(raw)) {
    if (!expandRequestDestinationEnv && hasEnvPlaceholder(rawKey) && isRequestDestinationKey(rawKey)) {
      warnIgnoredRequestDestinationEnv(filePath, rawKey, warnings)
      continue
    }
    if (!expandAuthValueEnv && hasEnvPlaceholder(rawKey) && isAuthValueKey(rawKey)) {
      warnIgnoredAuthValueEnv(filePath, rawKey, warnings)
      continue
    }
    const key = substituteEnv(rawKey, env, warnings)
    if (!expandRequestDestinationEnv && hasEnvPlaceholder(rawKey) && isRequestDestinationKey(key)) {
      warnIgnoredRequestDestinationEnv(filePath, rawKey, warnings)
      continue
    }
    if (!expandAuthValueEnv && hasEnvPlaceholder(rawKey) && isAuthValueKey(key)) {
      warnIgnoredAuthValueEnv(filePath, rawKey, warnings)
      continue
    }
    let value: unknown = rawValue
    if (typeof rawValue === 'string') {
      if (!expandRequestDestinationEnv && hasEnvPlaceholder(rawValue) && isRequestDestinationValueKey(key)) {
        warnIgnoredRequestDestinationEnv(filePath, key, warnings)
        continue
      }
      if (!expandAuthValueEnv && hasEnvPlaceholder(rawValue) && isAuthValueKey(key)) {
        warnIgnoredAuthValueEnv(filePath, key, warnings)
        continue
      }
      value = substituteEnv(rawValue, env, warnings)
    }

    // Only keep auth/registry related keys
    if (isNpmrcReadableKey(key)) {
      // A relative `cafile=` resolves against the .npmrc's directory rather
      // than process.cwd(), so `pnpm --dir <project>` from a different cwd
      // still finds it. See https://github.com/pnpm/pnpm/issues/11624.
      if (key === 'cafile' && typeof value === 'string' && value !== '' && !path.isAbsolute(value)) {
        value = path.resolve(npmrcDir, value)
      }
      result[key] = value
    }
  }
  return rescopeUnscopedCreds(result, filePath, warnings)
}

function isRequestDestinationKey (key: string): boolean {
  return isRegistryKey(key) || key.startsWith('//')
}

function isRequestDestinationValueKey (key: string): boolean {
  return isRegistryKey(key) || key === 'https-proxy' || key === 'http-proxy' || key === 'proxy'
}

function isRegistryKey (key: string): boolean {
  return key === 'registry' || (key.startsWith('@') && key.endsWith(':registry'))
}

const AUTH_VALUE_KEYS = ['_authToken', '_auth', '_password', 'username', 'tokenHelper', 'cert', 'key'] as const
const AUTH_VALUE_KEY_SUFFIXES = AUTH_VALUE_KEYS.map(key => `:${key}`)

function isAuthValueKey (key: string): boolean {
  return (AUTH_VALUE_KEYS as readonly string[]).includes(key) || AUTH_VALUE_KEY_SUFFIXES.some(suffix => key.endsWith(suffix))
}

function hasEnvPlaceholder (value: string): boolean {
  return /\$\{[^}]+\}/.test(value)
}

const DOCS_URL = 'https://pnpm.io/npmrc'

// The key embedded in the suggested `pnpm config set` command comes from a
// repository-controlled .npmrc. A shell expands `$(...)`, backticks and `$VAR`
// even inside double quotes, so suggesting a runnable command built from an
// arbitrary key would turn this warning into a copy-paste command-injection
// vector. Only emit the runnable example for keys made up entirely of
// shell-inert characters — which covers every real registry/auth key
// (`//host/:_authToken`, `@scope:registry`, `registry`, `https-proxy`, …).
const SHELL_SAFE_KEY = /^[\w@.:/-]+$/

function configSetExample (key: string): string {
  return SHELL_SAFE_KEY.test(key) ? ` (for example, run: pnpm config set "${key}" <value>)` : ''
}

function warnIgnoredRequestDestinationEnv (filePath: string, key: string, warnings: string[]): void {
  warnings.push(`Ignored project-level request destination "${key}" in "${filePath}": ` +
    'environment variables are not expanded in registry or proxy URLs that come from a project .npmrc, ' +
    'because that file is committed to the repository and a malicious value could redirect requests or leak secrets. ' +
    'Move this setting to a trusted source that pnpm still expands — put it in your user-level ~/.npmrc, ' +
    `or set it with pnpm config set${configSetExample(key)}. ` +
    `If the value is not secret, you can also write it literally in the project .npmrc. See ${DOCS_URL}`)
}

function warnIgnoredAuthValueEnv (filePath: string, key: string, warnings: string[]): void {
  warnings.push(`Ignored project-level auth setting "${key}" in "${filePath}": ` +
    'environment variables are not expanded in registry credentials that come from a project .npmrc, ' +
    'because that file is committed to the repository and could leak the secret to an attacker-controlled registry. ' +
    'Move this credential to a trusted source that pnpm still expands — put the line in your user-level ~/.npmrc, ' +
    `or set it with pnpm config set${configSetExample(key)}. See ${DOCS_URL}`)
}

// Rewrite any unscoped per-registry keys in `source` to their URL-scoped
// equivalents (`//host[:port]/path/:<key>=...`) using `source.registry` —
// or the builtin default registry if the source doesn't declare its own.
// This pins each layer's credential, client certificate, or CA setting to
// the registry that layer named (or the implicit npmjs default), so a
// later layer overriding `registry=` cannot pull a setting authored for
// one host along to a different host. A URL-scoped key for the same
// registry already present in `source` wins; we never overwrite an
// explicit scoped value.
//
// Each rewrite triggers a deprecation warning so users migrate to writing
// the URL-scoped form directly. npm has rejected unscoped credentials
// outright since `npm@9` (`ERR_INVALID_AUTH`).
function rescopeUnscopedCreds (
  source: Record<string, unknown>,
  sourceLabel: string,
  warnings: string[]
): Record<string, unknown> {
  // Bail early if there's nothing to rescope. This skips the nerfDart call
  // when a source like the builtin pnpmrc has only a `registry=` line —
  // rescoping there would do nothing anyway.
  if (!UNSCOPED_RESCOPABLE_KEYS.some(key => key in source)) {
    return source
  }
  const rawRegistry = typeof source.registry === 'string' && source.registry !== '' ? source.registry : null
  const fallbackRegistry = rawRegistry ?? npmDefaults.registry
  let nerfedRegistry: string
  try {
    nerfedRegistry = nerfDart(normalizeRegistryUrl(fallbackRegistry))
  } catch {
    // `registry=` resolved to something `URL` can't parse — often an
    // unresolved `${VAR}` placeholder that left the string empty. Drop the
    // unscoped keys (a bare token is unsafe to bind anywhere) and warn.
    const dropped = UNSCOPED_RESCOPABLE_KEYS.filter(key => key in source)
    for (const key of dropped) delete source[key]
    warnings.push(`Unscoped per-registry settings (${dropped.join(', ')}) in "${sourceLabel}" were ignored: ` +
      `the source's "registry" value (${JSON.stringify(source.registry)}) is not a parseable URL, so pnpm cannot pin them anywhere safe. ` +
      'Write them URL-scoped (e.g. "//registry.example.com/:_authToken=...") to send them to a specific registry.')
    return source
  }
  const rescoped: string[] = []
  for (const key of UNSCOPED_RESCOPABLE_KEYS) {
    if (!(key in source)) continue
    const scopedKey = `${nerfedRegistry}:${key}`
    if (!(scopedKey in source)) {
      source[scopedKey] = source[key]
    }
    delete source[key]
    rescoped.push(key)
  }
  if (rescoped.length > 0) {
    warnings.push(`Unscoped per-registry settings (${rescoped.join(', ')}) in "${sourceLabel}" are deprecated. ` +
      `pnpm pinned them to "${nerfedRegistry}" for this run, but a future release will stop supporting unscoped per-registry settings. ` +
      `Write them as "${nerfedRegistry}:${rescoped[0]}=..." instead.`)
  }
  return source
}

// Use the lossy variant so unresolved `${VAR}` placeholders become '' (each
// recorded as a warning) instead of throwing. Critical for the OIDC case in
// https://github.com/pnpm/pnpm/issues/11513 — leaving the literal `${VAR}` in
// an auth value would be sent verbatim as a bearer token. Resolvable
// placeholders and `${VAR-default}` / `${VAR:-default}` fallbacks elsewhere
// in the same string still expand normally.
function substituteEnv (value: string, env: Record<string, string | undefined>, warnings: string[]): string {
  const { value: substituted, unresolved } = envReplaceLossy(value, env)
  for (const placeholder of unresolved) {
    warnings.push(`Failed to replace env in config: ${placeholder}`)
  }
  return substituted
}

function normalizePath (p: string | undefined): string | undefined {
  if (p == null) return undefined
  if (p.startsWith('~/') || p.startsWith('~\\')) {
    p = path.join(os.homedir(), p.slice(2))
  }
  return path.resolve(p)
}

function isErrorWithCode (err: unknown, code: string): boolean {
  return err != null && typeof err === 'object' && 'code' in err && err.code === code
}

/**
 * Find the nearest directory containing package.json, node_modules,
 * or pnpm-workspace.yaml by walking up from startDir.
 * Ported from @pnpm/npm-conf/lib/util.js findPrefix.
 */
export function findLocalPrefix (startDir: string): string {
  let name = path.resolve(startDir)

  let walkedUp = false
  while (path.basename(name) === 'node_modules') {
    name = path.dirname(name)
    walkedUp = true
  }

  if (walkedUp) {
    return name
  }

  return findPrefixUp(name, name)
}

function findPrefixUp (name: string, original: string): string {
  const driveRootRegex = /^[a-z]:[/\\]?$/i
  if (name === '/' || (process.platform === 'win32' && driveRootRegex.test(name))) {
    return original
  }

  try {
    const files = fs.readdirSync(name)
    if (
      files.includes('node_modules') ||
      files.includes('package.json') ||
      files.includes('package.json5') ||
      files.includes('package.yaml') ||
      files.includes('pnpm-workspace.yaml')
    ) {
      return name
    }

    const dirname = path.dirname(name)
    if (dirname === name) {
      return original
    }

    return findPrefixUp(dirname, original)
  } catch (err: unknown) {
    if (name === original) {
      if (isErrorWithCode(err, 'ENOENT')) {
        return original
      }
      throw err
    }
    return original
  }
}

/**
 * If cafile is set in any layer, read it and set ca.
 * Replicates the behavior of @pnpm/network.ca-file's readCAFileSync:
 * splits on '-----END CERTIFICATE-----' and re-appends the delimiter.
 */
function loadCAFile (layers: Array<Record<string, unknown>>): void {
  let cafile: string | undefined
  for (const layer of layers) {
    if (typeof layer.cafile === 'string') {
      cafile = layer.cafile
      break
    }
  }
  if (!cafile) return

  try {
    const contents = fs.readFileSync(cafile, 'utf8')
    const delim = '-----END CERTIFICATE-----'
    const cas = contents
      .split(delim)
      .filter(ca => ca.trim().length > 0)
      .map(ca => `${ca.trimStart()}${delim}`)
    if (cas.length === 0) return
    for (const layer of layers) {
      if (typeof layer.cafile === 'string') {
        layer.ca = cas
        break
      }
    }
  } catch {
    // Ignore errors reading CA file (e.g., ENOENT)
  }
}
