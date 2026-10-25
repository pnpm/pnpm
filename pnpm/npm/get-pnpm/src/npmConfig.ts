import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { DEFAULT_REGISTRY, normalizeRegistry, type RequestHeaders } from './registry.js'

interface RegistryConfigOptions {
  cwd?: string
  env?: NodeJS.ProcessEnv
  execPath?: string
  homeDir?: string
  platform?: NodeJS.Platform
}

interface RegistryToken {
  pathname: string
  token: string
}

interface NpmrcCandidate {
  ignoreUnreadable: boolean
  path: string
}

/** The settings of one config source, in file order. */
export interface ConfigSource {
  /** What a relative `cafile` in this source is resolved against. */
  dir: string
  entries: Array<[string, string]>
}

export interface RegistryConfig {
  authPath?: string
  /** PEM certificates of the CAs the npm config names for `registry`. */
  ca?: string[]
  headers?: RequestHeaders
  registry: string
}

export function registryConfigFromEnv (opts: RegistryConfigOptions = {}): RegistryConfig {
  const env = opts.env ?? process.env
  const cwd = opts.cwd ?? process.cwd()
  const rawRegistry = env.npm_config_registry ?? env.NPM_CONFIG_REGISTRY ?? DEFAULT_REGISTRY
  const embedded = registryAndEmbeddedAuth(rawRegistry)
  const registry = embedded.registry
  // Highest precedence first, as npm layers them.
  const sources = [
    envSource(env, cwd),
    ...[
      ...projectNpmrcs(cwd, env),
      userNpmrc(opts, env),
      globalNpmrc(opts, env),
    ].map((candidate) => readNpmrc(candidate, env)),
  ]
  const ca = certificateAuthorities(registry, sources)
  const auth = embedded.headers != null ? embedded : tokenAuth(registry, sources)
  return { ...auth, ...(ca.length > 0 ? { ca } : {}) }
}

function tokenAuth (registry: string, sources: ConfigSource[]): RegistryConfig {
  const token = mostSpecificToken(sources.map((source) => tokenForRegistry(registry, source.entries)))
  return token == null
    ? { registry }
    : {
      registry,
      authPath: token.pathname,
      headers: { authorization: `Bearer ${token.token}` },
    }
}

function registryAndEmbeddedAuth (rawRegistry: string): RegistryConfig {
  const url = new URL(normalizeRegistry(rawRegistry))
  if (!url.username && !url.password) return { registry: url.href }

  const username = decodeURIComponent(url.username)
  const password = decodeURIComponent(url.password)
  url.username = ''
  url.password = ''
  return {
    registry: url.href,
    authPath: normalizedPath(url.pathname),
    headers: {
      authorization: `Basic ${Buffer.from(`${username}:${password}`).toString('base64')}`,
    },
  }
}

/**
 * The `npm_config_*` variables. npm sets them for the programs it runs, a
 * `ca` array joined into one value.
 */
function envSource (env: NodeJS.ProcessEnv, cwd: string): ConfigSource {
  const entries: Array<[string, string]> = []
  for (const [name, value] of Object.entries(env)) {
    if (value == null || !name.toLowerCase().startsWith('npm_config_')) continue
    const key = name.slice('npm_config_'.length)
    // npm's own names are lowercase; only a URL-scoped key keeps its case.
    entries.push([key.startsWith('//') ? key : key.toLowerCase(), value])
  }
  return { dir: cwd, entries }
}

function readNpmrc (npmrc: NpmrcCandidate, env: NodeJS.ProcessEnv): ConfigSource {
  const dir = path.dirname(npmrc.path)
  const text = readNpmrcText(npmrc)
  if (text == null) return { dir, entries: [] }

  const entries: Array<[string, string]> = []
  for (const rawLine of text.split(/\r?\n/)) {
    const entry = parseNpmrcLine(rawLine.trim(), env)
    if (entry != null) entries.push(entry)
  }
  return { dir, entries }
}

/** The text of `npmrc`, or `undefined` when there is none to read. */
function readNpmrcText (npmrc: NpmrcCandidate): string | undefined {
  try {
    return fs.readFileSync(npmrc.path, 'utf8')
  } catch (err) {
    if (isErrno(err, 'ENOENT')) return undefined
    if (npmrc.ignoreUnreadable && isUnreadableNpmrcError(err)) return undefined
    throw err
  }
}

function parseNpmrcLine (line: string, env: NodeJS.ProcessEnv): [string, string] | undefined {
  if (!line || line.startsWith('#') || line.startsWith(';')) return undefined
  const separator = line.indexOf('=')
  if (separator === -1) return undefined
  const key = line.slice(0, separator).trim()
  const value = expandEnv(stripQuotes(line.slice(separator + 1).trim()), env)
  return value == null ? undefined : [key, value]
}

function tokenForRegistry (registry: string, entries: Array<[string, string]>): RegistryToken | undefined {
  const registryUrl = new URL(registry)
  let best: RegistryToken | undefined

  for (const [key, token] of entries) {
    if (token === '') continue
    const scope = authTokenScope(key, registryUrl.protocol)
    if (scope == null || !scopeCovers(scope, registryUrl)) continue
    // A longer prefix wins; at equal length, the later entry does.
    if (best == null || scope.pathname.length >= best.pathname.length) {
      best = { pathname: scope.pathname, token }
    }
  }

  return best
}

function mostSpecificToken (candidates: Array<RegistryToken | undefined>): RegistryToken | undefined {
  let best: RegistryToken | undefined
  for (const candidate of candidates) {
    if (candidate == null) continue
    if (best == null || candidate.pathname.length > best.pathname.length) best = candidate
  }
  return best
}

function authTokenScope (key: string, protocol: string): { host: string, pathname: string } | undefined {
  return registryScope(key, ':_authtoken', protocol)
}

/**
 * The CA certificates the config adds for `registry`: those of the
 * highest-precedence source that sets a top-level `ca`, `ca[]`, or `cafile`,
 * and those scoped to the longest `//host/path/` prefix of `registry`. A
 * `cafile` that cannot be read adds nothing, as in pnpm.
 */
export function certificateAuthorities (registry: string, sources: ConfigSource[]): string[] {
  const topLevel = sources.find((source) => source.entries.some(([key]) => CA_SETTINGS.includes(key)))
  const settings: CaSetting[] = (topLevel?.entries ?? [])
    .filter(([key]) => CA_SETTINGS.includes(key))
    .map(([setting, value]) => ({ dir: topLevel!.dir, setting, value }))
  settings.push(...scopedCaSettings(registry, sources))
  return settings.flatMap(({ dir, setting, value }) => pemCertificates(
    setting === 'cafile' ? readCaFile(path.resolve(dir, value)) : value.replaceAll('\\n', '\n')
  ))
}

const CA_SETTINGS = ['ca', 'ca[]', 'cafile']

interface CaSetting {
  dir: string
  setting: string
  value: string
}

/** A longer prefix wins; at equal length, the higher-precedence source does. */
function scopedCaSettings (registry: string, sources: ConfigSource[]): CaSetting[] {
  const registryUrl = new URL(registry)
  const scoped = sources.flatMap((source, sourceIndex) => source.entries.flatMap(([key, value]) => {
    const scope = value === '' ? undefined : caSettingScope(key, registryUrl)
    return scope == null ? [] : [{ sourceIndex, dir: source.dir, value, ...scope }]
  }))
  const longest = Math.max(...scoped.map(({ pathname }) => pathname.length))
  const winner = scoped.find(({ pathname }) => pathname.length === longest)
  return scoped.filter(({ sourceIndex, pathname }) => sourceIndex === winner?.sourceIndex && pathname.length === longest)
}

/** The CA setting `key` scopes to a prefix of `registryUrl`, if it is one. */
function caSettingScope (key: string, registryUrl: URL): { pathname: string, setting: string } | undefined {
  for (const setting of CA_SETTINGS) {
    const scope = registryScope(key, `:${setting}`, registryUrl.protocol)
    if (scope == null) continue
    return scopeCovers(scope, registryUrl) ? { pathname: scope.pathname, setting } : undefined
  }
  return undefined
}

function scopeCovers (scope: { host: string, pathname: string }, registryUrl: URL): boolean {
  return scope.host === registryUrl.host.toLowerCase() && registryUrl.pathname.startsWith(scope.pathname)
}

function readCaFile (file: string): string {
  try {
    return fs.readFileSync(file, 'utf8')
  } catch {
    return ''
  }
}

function pemCertificates (text: string): string[] {
  return text.match(/-----BEGIN CERTIFICATE-----[\s\S]+?-----END CERTIFICATE-----/g) ?? []
}

function registryScope (key: string, suffix: string, protocol: string): { host: string, pathname: string } | undefined {
  const lower = key.toLowerCase()
  if (!lower.endsWith(suffix)) return undefined
  const rawScope = key.slice(0, -suffix.length)
  if (!rawScope.startsWith('//')) return undefined

  try {
    const url = new URL(`${protocol}${rawScope}`)
    return {
      host: url.host.toLowerCase(),
      pathname: normalizedPath(url.pathname),
    }
  } catch {
    return undefined
  }
}

function projectNpmrcs (cwd: string, env: NodeJS.ProcessEnv): NpmrcCandidate[] {
  const configured = env.npm_config_local_prefix ?? env.NPM_CONFIG_LOCAL_PREFIX
  if (configured != null) {
    return [{ path: path.join(configured, '.npmrc'), ignoreUnreadable: false }]
  }

  const current = path.resolve(cwd)
  const root = findProjectRoot(current)
  const paths = [path.join(current, '.npmrc')]
  if (root !== current) paths.push(path.join(root, '.npmrc'))
  return paths.map((npmrcPath) => ({ path: npmrcPath, ignoreUnreadable: true }))
}

function findProjectRoot (cwd: string): string {
  const start = path.resolve(cwd)
  let current = start
  while (true) {
    if (fs.existsSync(path.join(current, 'package.json'))) return current
    const parent = path.dirname(current)
    if (parent === current) return start
    current = parent
  }
}

function userNpmrc (opts: RegistryConfigOptions, env: NodeJS.ProcessEnv): NpmrcCandidate {
  const configured = env.npm_config_userconfig ?? env.NPM_CONFIG_USERCONFIG
  return configured != null
    ? { path: configured, ignoreUnreadable: false }
    : { path: path.join(npmHomeDir(opts, env), '.npmrc'), ignoreUnreadable: true }
}

function globalNpmrc (opts: RegistryConfigOptions, env: NodeJS.ProcessEnv): NpmrcCandidate {
  const configured = env.npm_config_globalconfig ?? env.NPM_CONFIG_GLOBALCONFIG
  if (configured != null) return { path: configured, ignoreUnreadable: false }

  const platform = opts.platform ?? process.platform
  const homeDir = npmHomeDir(opts, env)
  const prefix = env.npm_config_prefix ??
    env.NPM_CONFIG_PREFIX ??
    (platform === 'win32'
      ? path.join(env.APPDATA ?? path.join(homeDir, 'AppData', 'Roaming'), 'npm')
      : path.dirname(path.dirname(opts.execPath ?? process.execPath)))
  return { path: path.join(prefix, 'etc', 'npmrc'), ignoreUnreadable: true }
}

function npmHomeDir (opts: RegistryConfigOptions, env: NodeJS.ProcessEnv): string {
  if (opts.homeDir != null) return opts.homeDir
  const platform = opts.platform ?? process.platform
  if (platform === 'win32') return env.USERPROFILE ?? env.HOME ?? os.homedir()
  return env.HOME ?? os.homedir()
}

function normalizedPath (pathname: string): string {
  return pathname.endsWith('/') ? pathname : `${pathname}/`
}

export function stripQuotes (value: string): string {
  if (value.length < 2) return value
  const first = value[0]
  const last = value[value.length - 1]
  return (first === last && (first === '"' || first === "'")) ? value.slice(1, -1) : value
}

function expandEnv (value: string, env: NodeJS.ProcessEnv): string | undefined {
  let missing = false
  // `${NAME?}` is npm's spelling for "empty when unset"; a bare `${NAME}` that
  // is unset drops the entry rather than sending the placeholder as a token.
  const expanded = value.replace(/\$\{([^${}?]+)(\?)?\}/g, (_match, name: string, optional?: string) => {
    const resolved = env[name]
    if (resolved != null) return resolved
    if (optional == null) missing = true
    return ''
  })
  return missing ? undefined : expanded
}

function isUnreadableNpmrcError (err: unknown): boolean {
  return ['EACCES', 'EISDIR', 'EPERM'].some((code) => isErrno(err, code))
}

function isErrno (err: unknown, code: string): err is NodeJS.ErrnoException {
  return err instanceof Error && 'code' in err && err.code === code
}
