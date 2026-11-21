import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { certificateAuthorities, type ConfigSource } from './npmConfig.js'
import { findUp } from './projectPin.js'
import { DEFAULT_REGISTRY, isUrlSecureForCredentials, normalizeRegistry, type ProxyEnv, type RequestHeaders } from './registry.js'
import { parseYamlDocument } from './yaml.js'

export interface PnpmRegistryConfigOptions {
  /**
   * Whether to read the project `.npmrc`. Pass it only while the download is
   * checked against npm's signature. The signature pins the bytes, so the
   * repository picks only where they come from. Unchecked, it would pick the
   * executable itself (GHSA-j2hc-m6cf-6jm8).
   */
  readProjectNpmrc: boolean
  cwd?: string
  env?: NodeJS.ProcessEnv
  homeDir?: string
  platform?: NodeJS.Platform
}

export interface PnpmRegistryConfig {
  registry: string
  /** Credentials for `registry`, sent only over HTTPS or to the local machine. */
  headers?: RequestHeaders
  /** The path the credentials are scoped to, such as `/org-a/npm/`. */
  authPath?: string
  /** PEM certificates of the CAs the `.npmrc` names for `registry`. */
  ca?: string[]
}

/**
 * The registry to download pnpm from, with the credentials and CA certificates
 * for it, read the way pnpm reads them for an `@pnpm` package.
 *
 * - Sources, highest first: the `_auth` of the global `config.yaml`, the
 *   project `.npmrc` (the workspace root's, or else the one in `cwd`), the
 *   global `auth.ini`, then the user `.npmrc`. Only top-level settings count.
 * - The registry is `@pnpm:registry`, then `registry` from the environment,
 *   then `registry`. The routes `_auth` implies (its `@pnpm` and `@` scopes)
 *   apply only where no `.npmrc` or `auth.ini` declares that registry.
 * - `${VAR}` placeholders are expanded in every source but the project
 *   `.npmrc`. A project `.npmrc` entry that has one is dropped, as pnpm drops
 *   it: a repository must not route the user's environment to a registry.
 * - Credentials are those of the `@pnpm` scope, then the registry-wide ones,
 *   for the longest `//host[:port]/path/` prefix of the registry, then the
 *   same without the port. They are scoped to that prefix's path.
 *
 * @throws if the global `config.yaml` cannot be read or parsed, as pnpm does.
 */
export function pnpmRegistryConfig (opts: PnpmRegistryConfigOptions): PnpmRegistryConfig {
  const env = opts.env ?? process.env
  const configDir = pnpmConfigDir(opts, env)
  const npmrcs = readNpmrcs(opts, env, configDir)
  const jsonAuth = readJsonAuth(readGlobalConfig(configDir)._auth, env)
  const sources = [{ dir: configDir, entries: jsonAuth.entries }, ...npmrcs]
  const registry = pnpmRegistry(mergedSettings(npmrcs), jsonAuth.routes, env)
  const ca = certificateAuthorities(registry, sources)
  return {
    registry,
    ...npmrcAuth(mergedSettings(sources), registry),
    ...(ca.length === 0 ? {} : { ca }),
  }
}

/** The `.npmrc`-shaped sources, highest first. */
function readNpmrcs (opts: PnpmRegistryConfigOptions, env: NodeJS.ProcessEnv, configDir: string): ConfigSource[] {
  const expandEntry: ParseEntry = (key, value) => [expandEnv(key, env), expandEnv(value, env)]
  return [
    ...(opts.readProjectNpmrc ? [readNpmrc(path.join(projectDir(opts.cwd ?? process.cwd()), '.npmrc'), projectEntry)] : []),
    readNpmrc(path.join(configDir, 'auth.ini'), expandEntry),
    readNpmrc(userNpmrcPath(env, opts.homeDir), expandEntry),
  ]
}

/** The settings of `sources`, highest first, each taken from the highest source that has it. */
function mergedSettings (sources: ConfigSource[]): Map<string, string> {
  return new Map([...sources].reverse().flatMap(({ entries }) => entries))
}

/** A route of `_auth` applies only where no `.npmrc` declares that registry. */
function pnpmRegistry (npmrc: Map<string, string>, routes: Map<string, string>, env: NodeJS.ProcessEnv): string {
  const route = (key: string, scope: string): string | undefined => npmrc.has(key) ? npmrc.get(key) : routes.get(scope)
  return normalizeRegistry(
    route('@pnpm:registry', '@pnpm') || settingFromEnv(env, 'registry') || route('registry', '@') || DEFAULT_REGISTRY
  )
}

export interface PnpmProxyConfigOptions {
  env?: NodeJS.ProcessEnv
  homeDir?: string
  platform?: NodeJS.Platform
}

/** The proxy keys as one source spells them. */
interface ProxySettings {
  httpsProxy?: unknown
  httpProxy?: unknown
  noProxy?: unknown
  noproxy?: unknown
  proxy?: unknown
}

/**
 * The proxy to download pnpm through, resolved the way pnpm resolves it for
 * `pnpm self-update` and the download of the version a project pins.
 *
 * - Each of `httpsProxy`, `httpProxy`, `noProxy`, `proxy`, and `noproxy`
 *   comes from the first source that names it, highest first: the
 *   `PNPM_CONFIG_*` variables, then the global `config.yaml`, then the user
 *   `.npmrc`. The project's `.npmrc` and `pnpm-workspace.yaml` do not count.
 * - An empty, `false`, or `null` value reads as unset. `proxy` then stands in
 *   for `httpsProxy`, and `noproxy` for `noProxy`, and after them
 *   `HTTPS_PROXY`, `HTTP_PROXY`, `PROXY`, and `NO_PROXY` apply.
 * - The HTTP proxy defaults to the HTTPS one. `proxy` set to `false` turns
 *   both off, the environment included.
 *
 * @throws if the global `config.yaml` cannot be read or parsed, as pnpm does.
 */
export function pnpmProxyConfig (opts: PnpmProxyConfigOptions = {}): ProxyEnv {
  const env = opts.env ?? process.env
  const npmrc = new Map(
    readNpmrc(userNpmrcPath(env, opts.homeDir), (key, value) => [expandEnv(key, env), expandEnv(value, env)]).entries
  )
  const overlays: ProxySettings[] = [
    {
      httpsProxy: pnpmSettingFromEnv(env, 'https_proxy'),
      httpProxy: pnpmSettingFromEnv(env, 'http_proxy'),
      noProxy: pnpmSettingFromEnv(env, 'no_proxy'),
      noproxy: pnpmSettingFromEnv(env, 'noproxy'),
      proxy: pnpmSettingFromEnv(env, 'proxy'),
    },
    readGlobalConfigProxySettings(opts, env),
    {
      httpsProxy: npmrc.get('https-proxy'),
      httpProxy: npmrc.get('http-proxy'),
      noProxy: npmrc.get('no-proxy'),
      noproxy: npmrc.get('noproxy'),
      proxy: npmrc.get('proxy'),
    },
  ]
  const legacyProxy = overlaid(overlays, 'proxy')
  const httpsProxySetting = proxyUrl(overlaid(overlays, 'httpsProxy')) ??
    (legacyProxy === false || legacyProxy === 'false' ? false : proxyUrl(legacyProxy))
  const httpsProxy = httpsProxySetting === false ? undefined : httpsProxySetting ?? envValue(env, 'https_proxy')
  const httpProxy = proxyUrl(overlaid(overlays, 'httpProxy')) ??
    (httpsProxySetting === false ? undefined : httpsProxy ?? envValue(env, 'http_proxy') ?? envValue(env, 'proxy'))
  const noProxy = noProxyValue(overlaid(overlays, 'noProxy')) ?? noProxyValue(overlaid(overlays, 'noproxy')) ?? envValue(env, 'no_proxy')
  return {
    ...(httpsProxy == null ? {} : { HTTPS_PROXY: httpsProxy }),
    ...(httpProxy == null ? {} : { HTTP_PROXY: httpProxy }),
    ...(noProxy == null ? {} : { NO_PROXY: noProxy }),
  }
}

function overlaid (overlays: ProxySettings[], key: keyof ProxySettings): unknown {
  return overlays.find((overlay) => overlay[key] !== undefined)?.[key]
}

function proxyUrl (value: unknown): string | undefined {
  return typeof value === 'string' && !UNSET_VALUES.has(value) ? value : undefined
}

/** `true` bypasses the proxy for every host, which Node.js spells `*`. */
function noProxyValue (value: unknown): string | undefined {
  return value === true ? '*' : proxyUrl(value)
}

const UNSET_VALUES = new Set(['', 'false', 'null'])

function pnpmSettingFromEnv (env: NodeJS.ProcessEnv, key: string): string | undefined {
  return env[`pnpm_config_${key}`] ?? env[`PNPM_CONFIG_${key.toUpperCase()}`]
}

function envValue (env: NodeJS.ProcessEnv, key: string): string | undefined {
  return env[key] ?? env[key.toUpperCase()]
}

function readGlobalConfigProxySettings (opts: PnpmProxyConfigOptions, env: NodeJS.ProcessEnv): ProxySettings {
  const { httpsProxy, httpProxy, noProxy, noproxy, proxy } = readGlobalConfig(pnpmConfigDir(opts, env)) as ProxySettings
  return {
    httpsProxy: expandEnvIn(httpsProxy, env),
    httpProxy: expandEnvIn(httpProxy, env),
    noProxy: expandEnvIn(noProxy, env),
    noproxy: expandEnvIn(noproxy, env),
    proxy: expandEnvIn(proxy, env),
  }
}

/**
 * The settings of the global `config.yaml` in `configDir`, none when there is
 * no such file.
 *
 * @throws if the file cannot be read or parsed, or holds no map, as pnpm does.
 */
function readGlobalConfig (configDir: string): Record<string, unknown> {
  const file = path.join(configDir, 'config.yaml')
  let settings: unknown
  try {
    settings = parseYamlDocument(fs.readFileSync(file, 'utf8'))
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') return {}
    throw new Error(`Could not read ${file}: ${(err as Error).message}`, { cause: err })
  }
  if (settings == null) return {}
  if (typeof settings !== 'object' || Array.isArray(settings)) {
    throw new Error(`${file} does not hold a map of settings.`)
  }
  return settings as Record<string, unknown>
}

interface JsonAuth {
  /** `.npmrc`-shaped credential entries, such as `//host/path/:_authToken`. */
  entries: Array<[string, string]>
  /** The registry each scope (`@` for the default one) is routed to. */
  routes: Map<string, string>
}

/**
 * The credentials and routes of the `_auth` setting: a map from registry URL
 * to a map from scope (`@` or `@org`) to `{ authToken }`. An entry pnpm would
 * reject is skipped, so it adds no credentials.
 */
function readJsonAuth (value: unknown, env: NodeJS.ProcessEnv): JsonAuth {
  const auth: JsonAuth = { entries: [], routes: new Map() }
  if (!isRecord(value)) return auth
  for (const [registryUrl, scopes] of Object.entries(value)) {
    const registry = jsonAuthRegistry(registryUrl)
    if (registry != null && isRecord(scopes)) addJsonAuthScopes(auth, registry, scopes, env)
  }
  return auth
}

function addJsonAuthScopes (
  auth: JsonAuth,
  registry: { href: string, nerfDart: string },
  scopes: Record<string, unknown>,
  env: NodeJS.ProcessEnv
): void {
  for (const [scope, creds] of Object.entries(scopes)) {
    const token = jsonAuthToken(creds)
    if (!isJsonAuthScope(scope) || token == null) continue
    auth.entries.push([`${registry.nerfDart}:${scope === '@' ? '' : `${scope}:`}_authToken`, expandEnv(token, env)])
    auth.routes.set(scope, registry.href)
  }
}

function jsonAuthRegistry (registryUrl: string): { href: string, nerfDart: string } | undefined {
  let url: URL
  try {
    url = new URL(normalizeRegistry(registryUrl))
  } catch {
    return undefined
  }
  if ((url.protocol !== 'https:' && url.protocol !== 'http:') || url.username || url.password || url.search || url.hash) return undefined
  return { href: url.href, nerfDart: `//${url.host}${url.pathname}` }
}

/** The `authToken` of `{ authToken }`, the only shape pnpm accepts. */
function jsonAuthToken (creds: unknown): string | undefined {
  if (!isRecord(creds) || Object.keys(creds).some((field) => field !== 'authToken')) return undefined
  return typeof creds.authToken === 'string' ? creds.authToken : undefined
}

function isJsonAuthScope (scope: string): boolean {
  return scope === '@' || (scope.length > 1 && scope.startsWith('@') && !scope.includes('/') && !scope.includes(':'))
}

function isRecord (value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value != null && !Array.isArray(value)
}

/** A null value leaves the setting to a lower source, as pnpm reads the file. */
function expandEnvIn (value: unknown, env: NodeJS.ProcessEnv): unknown {
  if (value === null) return undefined
  return typeof value === 'string' ? expandEnv(value, env) : value
}

/** The directory pnpm keeps the global `config.yaml` and `auth.ini` in. */
function pnpmConfigDir (opts: { homeDir?: string, platform?: NodeJS.Platform }, env: NodeJS.ProcessEnv): string {
  const platform = opts.platform ?? process.platform
  const dirFromEnv = (name: string): string | undefined => {
    const value = env[name]
    return value && platform === 'win32' ? expandWindowsEnv(name, value, env) : value || undefined
  }
  const xdgConfigHome = dirFromEnv('XDG_CONFIG_HOME')
  if (xdgConfigHome) return path.join(xdgConfigHome, 'pnpm')
  const homeDir = opts.homeDir ?? os.homedir()
  if (platform === 'darwin') return path.join(homeDir, 'Library/Preferences/pnpm')
  const localAppData = platform === 'win32' ? dirFromEnv('LOCALAPPDATA') : undefined
  if (localAppData) return path.join(localAppData, 'pnpm/config')
  return path.join(homeDir, '.config/pnpm')
}

/** The longest value Windows allows in an environment variable. */
const MAX_WINDOWS_ENV_LENGTH = 32_767

/**
 * Expands the `%NAME%` references Windows leaves in a value it did not expand
 * itself, as pnpm does before using the value as a directory.
 *
 * @throws if a reference names no variable, as pnpm does.
 */
function expandWindowsEnv (name: string, value: string, env: NodeJS.ProcessEnv): string {
  const lookup = (varName: string): string | undefined => {
    const key = Object.keys(env).find((candidate) => candidate.toUpperCase() === varName.toUpperCase())
    return key == null ? undefined : env[key]
  }
  let current = value
  // Bounded, because a variable can reference itself, even twice over.
  for (let round = 0; round < 32 && current.includes('%'); round++) {
    const next = current.replace(/%([^%]+)%/g, (reference, varName: string) => lookup(varName) ?? reference)
    if (next === current || next.length > MAX_WINDOWS_ENV_LENGTH) break
    current = next
  }
  const unexpanded = /%[^%]+%/.exec(current)
  if (unexpanded != null) {
    throw new Error(`${name} contains an unexpanded environment variable: ${unexpanded[0]}`)
  }
  return current
}

type ParseEntry = (key: string, value: string, file: string) => [string, string] | undefined

function settingFromEnv (env: NodeJS.ProcessEnv, key: string): string | undefined {
  const upper = key.toUpperCase()
  return env[`pnpm_config_${key}`] || env[`PNPM_CONFIG_${upper}`] ||
    env[`npm_config_${key}`] || env[`NPM_CONFIG_${upper}`] || undefined
}

function userNpmrcPath (env: NodeJS.ProcessEnv, homeDir: string | undefined): string {
  return settingFromEnv(env, 'userconfig') ?? path.join(homeDir ?? os.homedir(), '.npmrc')
}

function projectEntry (key: string, value: string, file: string): [string, string] | undefined {
  if (!key.includes('${') && !value.includes('${')) return [key, value]
  if (key.startsWith('//') || key === 'registry' || key.endsWith(':registry')) {
    console.warn(`Ignored ${key} from ${file}: a project .npmrc cannot use environment variables in registry settings or credentials.`)
  }
  return undefined
}

function projectDir (cwd: string): string {
  const start = path.resolve(cwd)
  return findUp(start, undefined, (dir) => fs.existsSync(path.join(dir, 'pnpm-workspace.yaml'))) ?? start
}

function readNpmrc (file: string, parseEntry: ParseEntry): ConfigSource {
  const source: ConfigSource = { dir: path.dirname(file), entries: [] }
  for (const [key, value] of topLevelSettings(readNpmrcText(file))) {
    const entry = parseEntry(key, value, file)
    if (entry != null) source.entries.push(entry)
  }
  return source
}

function readNpmrcText (file: string): string {
  try {
    return fs.readFileSync(file, 'utf8')
  } catch (err) {
    const { code } = err as NodeJS.ErrnoException
    if (code !== 'ENOENT' && code !== 'EISDIR') {
      console.warn(`Could not read ${file}, so its settings are not used: ${(err as Error).message}`)
    }
    return ''
  }
}

function topLevelSettings (text: string): Array<[string, string]> {
  const settings: Array<[string, string]> = []
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim()
    // Settings under a `[section]` are not top-level ones.
    if (line.startsWith('[')) break
    const setting = line.startsWith(';') || line.startsWith('#') ? undefined : splitSetting(line)
    if (setting != null) settings.push(setting)
  }
  return settings
}

/**
 * The key and value of a `key=value` line. A JSON-quoted key may hold `=`,
 * as `pnpm login` writes one that does.
 */
function splitSetting (line: string): [string, string] | undefined {
  const keyEnd = line.startsWith('"') ? jsonStringEnd(line) : undefined
  const separator = keyEnd == null ? line.indexOf('=') : line.indexOf('=', keyEnd)
  if (separator === -1 || (keyEnd != null && line.slice(keyEnd, separator).trim() !== '')) return undefined
  return [decodeIniValue(line.slice(0, separator).trim()), decodeIniValue(line.slice(separator + 1).trim())]
}

/** The index just past the JSON string `line` starts with, if it is closed. */
function jsonStringEnd (line: string): number | undefined {
  for (let index = 1; index < line.length; index++) {
    if (line[index] === '\\') index++
    else if (line[index] === '"') return index + 1
  }
  return undefined
}

/** Unwraps a single-quoted value and decodes a JSON-quoted one, as pnpm does. */
function decodeIniValue (value: string): string {
  if (value.length < 2) return value
  if (value.startsWith("'") && value.endsWith("'")) return value.slice(1, -1)
  if (!value.startsWith('"') || !value.endsWith('"')) return value
  try {
    return JSON.parse(value) as string
  } catch {
    return value
  }
}

/**
 * Replaces `${NAME}`, and npm's `${NAME?}`, with the variable, or with nothing
 * when it is unset. `${NAME-fallback}` falls back when the variable is unset,
 * and `${NAME:-fallback}` also when it is empty, as in pnpm. As in pnpm, an odd
 * number of backslashes before `$` keeps the placeholder literal, and the
 * backslashes are halved.
 */
function expandEnv (value: string, env: NodeJS.ProcessEnv): string {
  return value.replace(/(?<!\\)(\\*)\$\{([^${}]+)\}/g, (match, escape: string, expression: string) => {
    if (escape.length % 2 === 1) return match.slice((escape.length + 1) / 2)
    return `${escape.slice(escape.length / 2)}${expandExpression(expression, env)}`
  })
}

function expandExpression (expression: string, env: NodeJS.ProcessEnv): string {
  const dash = expression.indexOf('-')
  if (dash === -1) return env[expression.endsWith('?') ? expression.slice(0, -1) : expression] ?? ''
  const fallbackOnEmpty = expression[dash - 1] === ':'
  const variable = env[expression.slice(0, fallbackOnEmpty ? dash - 1 : dash)]
  const fallback = expression.slice(dash + 1)
  return (fallbackOnEmpty ? variable || undefined : variable) ?? fallback
}

function npmrcAuth (settings: Map<string, string>, registry: string): Pick<PnpmRegistryConfig, 'headers' | 'authPath'> {
  const url = new URL(registry)
  if (!isUrlSecureForCredentials(url)) return {}
  const prefixes = registryPrefixes(url)
  for (const scope of [':@pnpm', '']) {
    for (const { prefix, pathname } of prefixes) {
      const authorization = npmrcAuthorization(settings, `${prefix}${scope}`)
      if (authorization != null) return { headers: { authorization }, authPath: pathname }
    }
  }
  return {}
}

/** Each `//host[:port]/path/` prefix of `url`, longest first, then the same without the port. */
function registryPrefixes (url: URL): Array<{ prefix: string, pathname: string }> {
  const prefixes: Array<{ prefix: string, pathname: string }> = []
  for (const host of new Set([url.host, url.hostname])) {
    let pathname = url.pathname
    while (true) {
      prefixes.push({ prefix: `//${host}${pathname}`, pathname })
      if (pathname === '/') break
      pathname = pathname.slice(0, pathname.lastIndexOf('/', pathname.length - 2) + 1)
    }
  }
  return prefixes
}

function npmrcAuthorization (settings: Map<string, string>, prefix: string): string | undefined {
  const token = settings.get(`${prefix}:_authToken`)
  if (token) return `Bearer ${token}`
  const auth = settings.get(`${prefix}:_auth`)
  if (auth) return `Basic ${auth}`
  const username = settings.get(`${prefix}:username`)
  const password = settings.get(`${prefix}:_password`)
  if (username && password) {
    return `Basic ${base64(`${username}:${Buffer.from(password, 'base64').toString('utf8')}`)}`
  }
  return undefined
}

function base64 (text: string): string {
  return Buffer.from(text, 'utf8').toString('base64')
}
