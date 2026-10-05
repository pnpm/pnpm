import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { envReplaceLossy } from '@pnpm/config.env-replace'
import { isError } from '@pnpm/error'
import normalizeRegistryUrl from 'normalize-registry-url'
import { readIniFileSync } from 'read-ini-file'

import { findLocalPrefix } from './findLocalPrefix.js'
import { type JsonAuthResult, readGlobalConfigAuth, readJsonAuthEnv } from './jsonAuth.js'
import { isNpmrcReadableKey } from './localConfig.js'
import { parseCAFileContents } from './parseCAFileContents.js'
import { redactRegistryUrl } from './registryUrlUserinfo.js'
import { rescopeUnscopedCreds } from './rescopeUnscopedCreds.js'

export { findLocalPrefix } from './findLocalPrefix.js'
export type { JsonAuthResult } from './jsonAuth.js'

export interface NpmrcConfigResult {
  /**
   * Merged auth/registry config from all sources.
   * Priority (lowest to highest): builtin < defaults < user < auth.ini < workspace < env (//-scoped + JSON auth) < CLI
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
  /** Parsed `_auth` (env var + global config yaml). See {@link JsonAuthResult}. */
  jsonAuth: JsonAuthResult
  /**
   * Scope→URL routes the `.npmrc` files declared through `registry=` and
   * `@scope:registry=`, keyed like `registriesByScope`. The builtin defaults
   * are not declarations, so they are absent.
   */
  declaredRegistries: Record<string, string>
  /** The same routes from the non-project `.npmrc` files, for the package-manager bootstrap. */
  trustedDeclaredRegistries: Record<string, string>
}

export interface LoadNpmrcConfigOpts {
  cliOptions: Record<string, unknown>
  defaultOptions: Record<string, unknown>
  /** Explicit working directory (from --dir flag) */
  dir?: string
  /** Workspace directory */
  workspaceDir?: string
  /** Skip the project `.npmrc` */
  ignoreProjectNpmrc?: boolean
  /** Custom path to user .npmrc (from npmrcAuthFile setting, overrides ~/.npmrc) */
  npmrcAuthFile?: string
  /** pnpm config directory (for pnpm auth file) */
  configDir: string
  /** Module directory for pnpm builtin rc */
  moduleDirname: string
  env?: Record<string, string | undefined>
  /**
   * The `_auth` value read from the **global** pnpm config yaml (the only
   * file source honored for `_auth`). Project `.npmrc` / `pnpm-workspace.yaml`
   * must not reach this — repo-controlled config may never supply auth.
   */
  globalConfigAuth?: unknown
  /** Receives the warnings as they are found, so they survive a throw. */
  warnings?: string[]
}

interface ReadAndFilterNpmrcOptions {
  expandAuthValueEnv?: boolean
  expandRequestDestinationEnv?: boolean
}

export function loadNpmrcConfig (opts: LoadNpmrcConfigOpts): NpmrcConfigResult {
  const warnings = opts.warnings ?? []
  const env = opts.env ?? process.env as Record<string, string | undefined>

  const localPrefix = opts.dir
    ? path.resolve(opts.dir)
    : findLocalPrefix(process.cwd())

  const { builtin, user, authIni, workspace, envScoped, jsonAuth, cli } = readNpmrcSources({ opts, env, localPrefix, warnings })

  // Handle cafile: expand to ca certs.
  // Priority: CLI > workspace > auth.ini > user > defaults
  loadCAFile([
    cli,
    workspace,
    authIni,
    user,
    opts.defaultOptions,
  ])

  return {
    mergedConfig: pickNpmrcReadableKeys([builtin, opts.defaultOptions, user, authIni, workspace, envScoped, jsonAuth.auth, cli]),
    rawConfig: {
      ...builtin,
      ...opts.defaultOptions,
      ...user,
      ...authIni,
      ...workspace,
      ...envScoped,
      ...jsonAuth.auth,
      ...cli,
    },
    // The env-scoped config is trusted (it comes from the environment, not the
    // repository), so it is included here while the workspace .npmrc is not.
    trustedConfig: pickNpmrcReadableKeys([builtin, opts.defaultOptions, user, authIni, envScoped, jsonAuth.auth, cli]),
    workspaceNpmrc: workspace,
    userConfig: user,
    localPrefix,
    warnings,
    jsonAuth,
    declaredRegistries: readDeclaredRegistries([user, authIni, workspace]),
    trustedDeclaredRegistries: readDeclaredRegistries([user, authIni]),
  }
}

interface ReadNpmrcSourcesContext {
  opts: LoadNpmrcConfigOpts
  env: Record<string, string | undefined>
  localPrefix: string
  warnings: string[]
}

interface NpmrcSources {
  builtin: Record<string, unknown>
  user: Record<string, unknown>
  authIni: Record<string, unknown>
  workspace: Record<string, unknown>
  envScoped: Record<string, unknown>
  jsonAuth: JsonAuthResult
  cli: Record<string, unknown>
}

function readNpmrcSources ({ opts, env, localPrefix, warnings }: ReadNpmrcSourcesContext): NpmrcSources {
  const userConfigPath = normalizePath(opts.npmrcAuthFile) ?? path.resolve(os.homedir(), '.npmrc')

  const workspaceNpmrcDir = opts.workspaceDir ?? localPrefix
  const workspaceNpmrcPath = path.resolve(workspaceNpmrcDir, '.npmrc')
  // When npmrcAuthFile explicitly points at the project .npmrc, the user has
  // opted in to trusting it — allow auth env expansion and suppress the warning.
  const workspaceIsTrustedAuthFile = userConfigPath === workspaceNpmrcPath
  const workspace = opts.ignoreProjectNpmrc
    ? {}
    : readAndFilterNpmrc(
      workspaceNpmrcPath,
      warnings,
      env,
      {
        expandAuthValueEnv: workspaceIsTrustedAuthFile,
        expandRequestDestinationEnv: workspaceIsTrustedAuthFile,
      }
    )

  const user = readAndFilterNpmrc(userConfigPath, warnings, env)

  const authIni = readAndFilterNpmrc(
    path.join(opts.configDir, 'auth.ini'),
    warnings,
    env
  )

  // Apply the same per-source rescope to CLI options so an unscoped
  // `--_authToken` follows the same trust rule as one written into an .npmrc.
  // We clone first to avoid mutating the caller's cliOptions object.
  const cli = rescopeUnscopedCreds({ ...opts.cliOptions }, '<command line>', warnings)

  // URL-scoped auth/registry settings supplied via `npm_config_//...` and
  // `pnpm_config_//...` environment variables. The registry a credential is
  // bound to is encoded in the (trusted) variable name, so unlike a project
  // `.npmrc` these cannot be redirected to another host by the repository —
  // making them a safe, file-free way to configure registry authentication.
  const envScoped = readUrlScopedEnvConfig(env)

  const jsonAuth = readJsonAuth(opts.globalConfigAuth, env, warnings)

  const builtin = readPnpmBuiltinConfig(opts.moduleDirname, warnings, env)

  return { builtin, user, authIni, workspace, envScoped, jsonAuth, cli }
}

// Structured `_auth` registry auth from two trusted, non-repo sources:
// the `pnpm_config__auth` env var (one JSON object, so it survives CI
// runners that silently drop env vars whose names contain `/`, `:`, or
// `.` — GitHub Actions, bash, zsh; see pnpm/pnpm#12314) and the `_auth`
// key of the global pnpm config yaml. The env var wins on conflict.
function readJsonAuth (globalConfigAuth: unknown, env: Record<string, string | undefined>, warnings: string[]): JsonAuthResult {
  const envJsonAuth = readJsonAuthEnv(env)
  const globalConfigJsonAuth = readGlobalConfigAuth(globalConfigAuth)
  const jsonAuth: JsonAuthResult = {
    auth: { ...globalConfigJsonAuth.auth, ...envJsonAuth.auth },
    registries: envJsonAuth.registries,
    fallbackRegistries: globalConfigJsonAuth.registries,
    defaultCandidates: envJsonAuth.defaultCandidates,
  }
  for (const [key, value] of Object.entries(jsonAuth.auth)) {
    jsonAuth.auth[key] = substituteEnv(value, env, { warnings, key, context: ' in _auth.authToken' })
  }
  return jsonAuth
}

function readPnpmBuiltinConfig (moduleDirname: string, warnings: string[], env: Record<string, string | undefined>): Record<string, unknown> {
  return {
    ...readAndFilterNpmrc(
      path.resolve(path.join(moduleDirname, 'pnpmrc')),
      warnings,
      env
    ),
    registry: 'https://registry.npmjs.org/',
    '@jsr:registry': 'https://npm.jsr.io/',
  }
}

function pickNpmrcReadableKeys (sources: Array<Record<string, unknown>>): Record<string, unknown> {
  const picked: Record<string, unknown> = {}
  for (const source of sources) {
    for (const [key, value] of Object.entries(source)) {
      if (isNpmrcReadableKey(key)) {
        picked[key] = value
      }
    }
  }
  return picked
}

/**
 * The scope→URL routes `sources` declare through `registry=` and
 * `@scope:registry=`, a later source overriding an earlier one. Whether a
 * registry was declared is a question about the key, not its value, so one
 * pinned to the builtin default is declared too. A value that is not a
 * string is not a route and is skipped.
 */
function readDeclaredRegistries (sources: Array<Record<string, unknown>>): Record<string, string> {
  const registries: Record<string, string> = {}
  for (const source of sources) {
    for (const [key, value] of Object.entries(source)) {
      if (typeof value !== 'string' || !isRegistryKey(key)) continue
      const scope = key === 'registry' ? 'default' : key.slice(0, -':registry'.length)
      registries[scope] = normalizeRegistryUrl(value)
    }
  }
  return registries
}

// Matches `npm_config_//...` and `pnpm_config_//...` env var names. The prefix is
// matched case-insensitively (as npm does), but the captured key keeps its
// original case because URL-scoped keys are case-sensitive (e.g. `:_authToken`).
const URL_SCOPED_ENV_RE = /^p?npm_config_(\/\/.+)$/i

// Collect URL-scoped settings (keys beginning with `//host...`, such as
// `//registry.npmjs.org/:_authToken`) from `npm_config_//...` and `pnpm_config_//...`
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
    // `tokenHelper` names an executable pnpm runs, so it must never be honored
    // from the environment. The TOKEN_HELPER_IN_PROJECT_CONFIG check in index.ts
    // validates against the trusted config, and that config already includes
    // this env-scoped layer — so admitting `//host/:tokenHelper` here would let
    // an env var pass the guard and silently run an arbitrary command. Drop it.
    if (key.endsWith(':tokenHelper')) continue
    const target = envKey.slice(0, 5).toLowerCase() === 'pnpm_' ? pnpmScoped : npmScoped
    target[key] = value
  }
  return { ...npmScoped, ...pnpmScoped }
}

function readAndFilterNpmrc (
  filePath: string,
  warnings: string[],
  env: Record<string, string | undefined>,
  opts: ReadAndFilterNpmrcOptions = {}
): Record<string, unknown> {
  const raw = readNpmrcFile(filePath, warnings)
  if (raw == null) return {}

  const ctx: NpmrcFilterContext = {
    filePath,
    warnings,
    env,
    expandAuthValueEnv: opts.expandAuthValueEnv ?? true,
    expandRequestDestinationEnv: opts.expandRequestDestinationEnv ?? true,
  }
  const npmrcDir = path.dirname(filePath)
  const result: Record<string, unknown> = {}
  for (const [rawKey, rawValue] of Object.entries(raw)) {
    const entry = readNpmrcEntry(ctx, rawKey, rawValue)
    // Only keep auth/registry related keys
    if (entry == null || !isNpmrcReadableKey(entry.key)) continue
    result[entry.key] = resolveCafilePath(entry, npmrcDir)
  }
  return rescopeUnscopedCreds(result, filePath, warnings)
}

function readNpmrcFile (filePath: string, warnings: string[]): Record<string, unknown> | undefined {
  try {
    return readIniFileSync(filePath) as Record<string, unknown>
  } catch (err: unknown) {
    if (isErrorWithCode(err, 'ENOENT') || isErrorWithCode(err, 'EISDIR')) {
      return undefined
    }
    warnings.push(`Issue while reading "${filePath}". ${isError(err) ? err.message : String(err)}`)
    return undefined
  }
}

interface NpmrcFilterContext {
  filePath: string
  warnings: string[]
  env: Record<string, string | undefined>
  expandAuthValueEnv: boolean
  expandRequestDestinationEnv: boolean
}

interface NpmrcEntry {
  key: string
  value: unknown
}

function readNpmrcEntry (ctx: NpmrcFilterContext, rawKey: string, rawValue: unknown): NpmrcEntry | undefined {
  const keyGate = { text: rawKey, reportedKey: rawKey, isRequestDestination: isRequestDestinationKey }
  if (skipsUnexpandedEnv(ctx, { ...keyGate, key: rawKey })) return undefined
  const key = substituteEnv(rawKey, ctx.env, { warnings: ctx.warnings, key: rawKey })
  if (skipsUnexpandedEnv(ctx, { ...keyGate, key })) return undefined
  if (typeof rawValue !== 'string') return { key, value: rawValue }
  if (skipsUnexpandedEnv(ctx, { text: rawValue, key, reportedKey: key, isRequestDestination: isRequestDestinationValueKey })) {
    return undefined
  }
  return { key, value: substituteEnv(rawValue, ctx.env, { warnings: ctx.warnings, key }) }
}

interface UnexpandedEnvGate {
  /** The raw text whose env placeholders would be expanded. */
  text: string
  /** The key that decides whether the text is a request destination or an auth value. */
  key: string
  /** The key named in the warning. */
  reportedKey: string
  isRequestDestination: (key: string) => boolean
}

/**
 * Whether an entry is dropped because it would expand an environment variable
 * into a request destination or an auth value that the source may not expand.
 * Warns about every dropped entry.
 */
function skipsUnexpandedEnv (ctx: NpmrcFilterContext, gate: UnexpandedEnvGate): boolean {
  if (!hasEnvPlaceholder(gate.text)) return false
  if (!ctx.expandRequestDestinationEnv && gate.isRequestDestination(gate.key)) {
    warnIgnoredRequestDestinationEnv(ctx.filePath, gate.reportedKey, ctx.warnings)
    return true
  }
  if (!ctx.expandAuthValueEnv && isAuthValueKey(gate.key)) {
    warnIgnoredAuthValueEnv(ctx.filePath, gate.reportedKey, ctx.warnings)
    return true
  }
  return false
}

// A relative `cafile=` resolves against the .npmrc's directory rather
// than process.cwd(), so `pnpm --dir <project>` from a different cwd
// still finds it. See https://github.com/pnpm/pnpm/issues/11624.
function resolveCafilePath ({ key, value }: NpmrcEntry, npmrcDir: string): unknown {
  if (key === 'cafile' && typeof value === 'string' && value !== '' && !path.isAbsolute(value)) {
    return path.resolve(npmrcDir, value)
  }
  return value
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

function warnIgnoredRequestDestinationEnv (filePath: string, key: string, warnings: string[]): void {
  warnings.push(`Ignored project-level request destination "${redactRegistryUrl(key)}" in "${filePath}": ` +
    'environment variables are not expanded in registry or proxy URLs that come from a project .npmrc, ' +
    'because that file is committed to the repository and a malicious value could redirect requests or leak secrets. ' +
    `If the value is not secret, you can also write it literally in the project .npmrc. See ${DOCS_URL}`)
}

function warnIgnoredAuthValueEnv (filePath: string, key: string, warnings: string[]): void {
  warnings.push(`Ignored project-level auth setting "${redactRegistryUrl(key)}" in "${filePath}": ` +
    'environment variables are not expanded in registry credentials that come from a project .npmrc, ' +
    'because that file is committed to the repository and could leak the secret to an attacker-controlled registry. ' +
    `See ${DOCS_URL}`)
}

// Use the lossy variant so unresolved `${VAR}` placeholders become '' (each
// recorded as a warning) instead of throwing. Critical for the OIDC case in
// https://github.com/pnpm/pnpm/issues/11513 — leaving the literal `${VAR}` in
// an auth value would be sent verbatim as a bearer token. Resolvable
// placeholders and `${VAR-default}` / `${VAR:-default}` fallbacks elsewhere
// in the same string still expand normally.
function substituteEnv (value: string, env: Record<string, string | undefined>, opts: { warnings: string[], key: string, context?: string }): string {
  const { warnings, key } = opts
  const authKey = AUTH_VALUE_KEYS.find(name => key === name || key.endsWith(`:${name}`))
  const context = opts.context ?? (authKey ? ` in .npmrc key "${authKey}"` : '')
  const { value: substituted, unresolved } = envReplaceLossy(value, withOptionalEnvPlaceholders(value, env))
  for (const placeholder of unresolved) {
    warnings.push(`Failed to replace env in config: ${placeholder}${context}`)
  }
  for (const placeholder of findEmptyEnvPlaceholders(value, env)) {
    warnings.push(`Failed to replace env in config: ${placeholder}${context}`)
  }
  return substituted
}

// npm's `${VAR?}` expands to VAR, or to '' without a warning when VAR is
// unset. envReplaceLossy reads the whole `VAR?` body as the variable name, so
// the lookup table gets a `VAR?` entry for each such placeholder. A name
// ending in `-` is left to envReplaceLossy's `${VAR-fallback}` form.
function withOptionalEnvPlaceholders (value: string, env: Record<string, string | undefined>): Record<string, string | undefined> {
  const names = Array.from(value.matchAll(/\$\{([^${}?]*[^${}?-])\?\}/g), ([, name]) => name)
  if (names.length === 0) return env
  const envWithOptional = { ...env }
  for (const name of names) {
    envWithOptional[`${name}?`] = env[name] ?? ''
  }
  return envWithOptional
}

function findEmptyEnvPlaceholders (value: string, env: Record<string, string | undefined>): string[] {
  const placeholders: string[] = []
  for (const match of value.matchAll(/(?<!\\)(\\*)\$\{([^${}]+)\}/g)) {
    const [, escapes, name] = match
    if ((escapes.length % 2) !== 0 || name.includes(':-') || name.includes('-')) continue
    if (env[name] === '') placeholders.push(`\${${name}}`)
  }
  return placeholders
}

function normalizePath (filePath: string | undefined): string | undefined {
  if (filePath == null) return undefined
  if (filePath.startsWith('~/') || filePath.startsWith('~\\')) {
    filePath = path.join(os.homedir(), filePath.slice(2))
  }
  return path.resolve(filePath)
}

function isErrorWithCode (err: unknown, code: string): boolean {
  return err != null && typeof err === 'object' && 'code' in err && err.code === code
}

/**
 * If cafile is set in any layer, read it and set ca.
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
    const cas = parseCAFileContents(contents)
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
