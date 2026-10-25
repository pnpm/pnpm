import fs from 'node:fs'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'

import { certificateAuthorities, type ConfigSource, stripQuotes } from './npmConfig.js'
import { findUp } from './projectPin.js'
import { DEFAULT_REGISTRY, normalizeRegistry, type RequestHeaders } from './registry.js'

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
}

export interface PnpmRegistryConfig {
  registry: string
  /** Credentials for `registry`, sent only over HTTPS or to the local machine. */
  headers?: RequestHeaders
  /** PEM certificates of the CAs the `.npmrc` names for `registry`. */
  ca?: string[]
}

/**
 * The registry to download pnpm from, with the credentials and CA certificates
 * for it, read the way pnpm reads them for an `@pnpm` package.
 *
 * - Sources, highest first: the project `.npmrc` (the workspace root's, or else
 *   the one in `cwd`), then the user `.npmrc`. Only top-level settings count.
 * - The registry is `@pnpm:registry`, then `registry` from the environment,
 *   then `registry`.
 * - `${VAR}` placeholders are expanded in the user `.npmrc` only. A project
 *   `.npmrc` entry that has one is dropped, as pnpm drops it: a repository
 *   must not route the user's environment to a registry.
 * - Credentials are those of the `@pnpm` scope, then the registry-wide ones,
 *   for the longest `//host[:port]/path/` prefix of the registry, then the
 *   same without the port.
 */
export function pnpmRegistryConfig (opts: PnpmRegistryConfigOptions): PnpmRegistryConfig {
  const env = opts.env ?? process.env
  const cwd = opts.cwd ?? process.cwd()
  const sources = [
    ...(opts.readProjectNpmrc ? [readNpmrc(path.join(projectDir(cwd), '.npmrc'), projectEntry)] : []),
    readNpmrc(userNpmrcPath(env, opts.homeDir), (key, value) => [expandEnv(key, env), expandEnv(value, env)]),
  ]
  const settings = new Map([...sources].reverse().flatMap(({ entries }) => entries))
  const registry = normalizeRegistry(
    settings.get('@pnpm:registry') || settingFromEnv(env, 'registry') || settings.get('registry') || DEFAULT_REGISTRY
  )
  const headers = npmrcHeaders(settings, registry)
  const ca = certificateAuthorities(registry, sources)
  return {
    registry,
    ...(headers == null ? {} : { headers }),
    ...(ca.length === 0 ? {} : { ca }),
  }
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
    const separator = line.indexOf('=')
    if (separator === -1 || line.startsWith(';') || line.startsWith('#')) continue
    settings.push([line.slice(0, separator).trim(), stripQuotes(line.slice(separator + 1).trim())])
  }
  return settings
}

function expandEnv (value: string, env: NodeJS.ProcessEnv): string {
  return value.replace(/\$\{([^${}]+)\}/g, (_match, name: string) => env[name] ?? '')
}

function npmrcHeaders (settings: Map<string, string>, registry: string): RequestHeaders | undefined {
  const url = new URL(registry)
  if (url.protocol !== 'https:' && !(url.protocol === 'http:' && isLoopback(url.hostname))) return undefined
  const prefixes = registryPrefixes(url)
  for (const scope of [':@pnpm', '']) {
    for (const prefix of prefixes) {
      const authorization = npmrcAuthorization(settings, `${prefix}${scope}`)
      if (authorization != null) return { authorization }
    }
  }
  return undefined
}

/** Each `//host[:port]/path/` prefix of `url`, longest first, then the same without the port. */
function registryPrefixes (url: URL): string[] {
  const prefixes: string[] = []
  for (const host of new Set([url.host, url.hostname])) {
    let pathname = url.pathname
    while (true) {
      prefixes.push(`//${host}${pathname}`)
      if (pathname === '/') break
      pathname = pathname.slice(0, pathname.lastIndexOf('/', pathname.length - 2) + 1)
    }
  }
  return prefixes
}

function isLoopback (hostname: string): boolean {
  return hostname === 'localhost' || hostname === '[::1]' ||
    (net.isIPv4(hostname) && hostname.startsWith('127.'))
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
