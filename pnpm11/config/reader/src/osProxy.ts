import { execFileSync } from 'node:child_process'

/**
 * Proxy URLs and bypass list read from the operating system.
 *
 * Strings are `host:port` when the system settings omit the URL scheme.
 * Callers prefix `http://` when they build a request agent.
 */
export interface OsProxy {
  httpsProxy?: string
  httpProxy?: string
  noProxy?: string
}

export interface ProxySourceInput {
  httpsProxy: unknown
  httpProxy: unknown
  proxy: unknown
  noProxy: unknown
  noproxy: unknown
  envHttpsProxy: string | undefined
  envHttpProxy: string | undefined
  envProxy: string | undefined
  envNoProxy: string | undefined
  os: OsProxy
}

export interface ResolvedProxySettings {
  httpsProxy: unknown
  httpProxy: unknown
  noProxy: unknown
}

/**
 * Config, then the environment, then the operating system.
 *
 * An empty environment variable is a value: it shadows the operating
 * system and resolves to no proxy. `proxy: false` does not fall through.
 * HTTPS filled only from the operating system does not hide a distinct
 * OS HTTP proxy.
 */
export function resolveProxyFromSources (input: ProxySourceInput): ResolvedProxySettings {
  const httpsWasUnset = !input.httpsProxy
  const legacyProxy = input.proxy === '' ? undefined : input.proxy
  const httpsFromConfigOrEnv = httpsWasUnset
    ? legacyProxy ?? input.envHttpsProxy
    : input.httpsProxy
  const httpsProxy = httpsWasUnset
    ? httpsFromConfigOrEnv ?? input.os.httpsProxy
    : input.httpsProxy

  let httpProxy = input.httpProxy
  if (!httpProxy) {
    const envHttpProxy = input.envHttpProxy ?? input.envProxy
    const httpFromUpper = httpsFromConfigOrEnv ?? envHttpProxy
    if (httpFromUpper != null && httpFromUpper !== '') {
      httpProxy = httpFromUpper
    } else if (httpFromUpper === '') {
      httpProxy = ''
    } else {
      const httpsFromOs = httpsProxy != null && httpsProxy !== '' && httpsProxy !== httpsFromConfigOrEnv
        ? httpsProxy
        : undefined
      httpProxy = input.os.httpProxy ?? httpsFromOs
    }
  }

  let noProxy = input.noProxy
  if (!noProxy) {
    const fromUpper = input.noproxy ?? input.envNoProxy
    noProxy = fromUpper ?? input.os.noProxy
  }

  return { httpsProxy, httpProxy, noProxy }
}

/**
 * The operating-system proxy for this process.
 *
 * Jest sets `JEST_WORKER_ID`. Tests that need a specific OS proxy pass it
 * in, so discovery here stays empty and does not read the machine.
 */
export function currentOsProxy (): OsProxy {
  if (process.env.JEST_WORKER_ID != null) return {}
  return readOsProxy()
}

function readOsProxy (): OsProxy {
  if (process.platform === 'win32') return readWindowsProxy()
  if (process.platform === 'darwin') return readMacosProxy()
  return {}
}

function readWindowsProxy (): OsProxy {
  const stdout = commandStdout('reg', [
    'query',
    'HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings',
  ])
  return stdout == null ? {} : parseWindowsInternetSettings(stdout)
}

function readMacosProxy (): OsProxy {
  const stdout = commandStdout('scutil', ['--proxy'])
  return stdout == null ? {} : parseMacosScutilProxy(stdout)
}

function commandStdout (program: string, args: string[]): string | undefined {
  try {
    return execFileSync(program, args, {
      encoding: 'utf8',
      timeout: 5000,
      windowsHide: true,
    })
  } catch {
    return undefined
  }
}

/**
 * Parse `reg query` output for the Internet Settings key.
 *
 * `ProxyEnable` of `0` or a missing value means no proxy, even when
 * `ProxyServer` is present. A server value without `=` applies to both
 * schemes. `http=` and `https=` entries are read on their own.
 */
export function parseWindowsInternetSettings (output: string): OsProxy {
  if (registryDword(output, 'ProxyEnable') === 0) return {}
  const server = registryValue(output, 'ProxyServer')?.trim() ?? ''
  const { httpProxy, httpsProxy } = splitProxyServer(server)
  const noProxy = joinBypassEntries((registryValue(output, 'ProxyOverride') ?? '').split(';'))
  return {
    httpProxy,
    httpsProxy,
    noProxy: noProxy === '' ? undefined : noProxy,
  }
}

/**
 * Parse `scutil --proxy` output.
 *
 * A scheme is used only when its enable flag is non-zero and a host is
 * present. `ExceptionsList` becomes the bypass list.
 */
export function parseMacosScutilProxy (output: string): OsProxy {
  let httpEnable = false
  let httpsEnable = false
  let httpHost: string | undefined
  let httpsHost: string | undefined
  let httpPort: number | undefined
  let httpsPort: number | undefined
  const exceptions: string[] = []
  let inExceptions = false

  for (const line of output.split('\n')) {
    const trimmed = line.trim()
    if (inExceptions) {
      if (trimmed === '}') {
        inExceptions = false
        continue
      }
      const separator = trimmed.indexOf(' : ')
      if (separator !== -1) {
        const entry = normalizeBypassEntry(trimmed.slice(separator + 3).trim())
        if (entry != null) exceptions.push(entry)
      }
      continue
    }
    if (trimmed.startsWith('ExceptionsList') && trimmed.includes('<array>')) {
      inExceptions = true
      continue
    }
    const separator = trimmed.indexOf(' : ')
    if (separator === -1) continue
    const key = trimmed.slice(0, separator).trim()
    const value = trimmed.slice(separator + 3).trim()
    switch (key) {
      case 'HTTPEnable':
        httpEnable = flagEnabled(value)
        break
      case 'HTTPSEnable':
        httpsEnable = flagEnabled(value)
        break
      case 'HTTPProxy':
        httpHost = nonEmpty(value)
        break
      case 'HTTPSProxy':
        httpsHost = nonEmpty(value)
        break
      case 'HTTPPort':
        httpPort = positivePort(value)
        break
      case 'HTTPSPort':
        httpsPort = positivePort(value)
        break
    }
  }

  return {
    httpProxy: enabledEndpoint(httpEnable, httpHost, httpPort),
    httpsProxy: enabledEndpoint(httpsEnable, httpsHost, httpsPort),
    noProxy: exceptions.length === 0 ? undefined : exceptions.join(','),
  }
}

function registryValue (output: string, name: string): string | undefined {
  for (const line of output.split('\n')) {
    const value = registryLineValue(line, name)
    if (value != null) return value
  }
  return undefined
}

function registryLineValue (line: string, name: string): string | undefined {
  if (!line.startsWith('    ')) return undefined
  const rest = line.slice(4)
  if (rest.length < name.length || rest.slice(0, name.length).toLowerCase() !== name.toLowerCase()) {
    return undefined
  }
  if (!rest.slice(name.length).startsWith('    ')) return undefined
  const afterName = rest.slice(name.length + 4)
  const typeEnd = afterName.indexOf('    ')
  if (typeEnd === -1) return undefined
  return afterName.slice(typeEnd + 4).trim()
}

function registryDword (output: string, name: string): number {
  const raw = registryValue(output, name)
  if (raw == null) return 0
  const hex = raw.startsWith('0x') ? raw.slice(2) : raw
  const parsed = Number.parseInt(hex, 16)
  return Number.isNaN(parsed) ? 0 : parsed
}

function splitProxyServer (server: string): { httpProxy?: string, httpsProxy?: string } {
  if (server === '') return {}
  if (!server.includes('=')) {
    return { httpProxy: server, httpsProxy: server }
  }
  let httpProxy: string | undefined
  let httpsProxy: string | undefined
  for (const entry of server.split(';')) {
    const separator = entry.indexOf('=')
    if (separator === -1) continue
    const scheme = entry.slice(0, separator).trim().toLowerCase()
    const value = entry.slice(separator + 1).trim()
    if (value === '') continue
    if (scheme === 'http') httpProxy = value
    if (scheme === 'https') httpsProxy = value
  }
  return { httpProxy, httpsProxy }
}

function joinBypassEntries (entries: string[]): string {
  return entries.map(normalizeBypassEntry).filter((entry): entry is string => entry != null).join(',')
}

function normalizeBypassEntry (entry: string): string | undefined {
  const trimmed = entry.trim()
  if (trimmed === '' || trimmed.toLowerCase() === '<local>') return undefined
  const stripped = trimmed.startsWith('*.') ? trimmed.slice(2) : trimmed
  return stripped === '' ? undefined : stripped
}

function flagEnabled (value: string): boolean {
  return value !== '' && value !== '0'
}

function nonEmpty (value: string): string | undefined {
  return value === '' ? undefined : value
}

function positivePort (value: string): number | undefined {
  const port = Number(value)
  if (!Number.isInteger(port) || port <= 0 || port > 65535) return undefined
  return port
}

function enabledEndpoint (enabled: boolean, host: string | undefined, port: number | undefined): string | undefined {
  if (!enabled || host == null) return undefined
  return port == null ? host : `${host}:${port}`
}
