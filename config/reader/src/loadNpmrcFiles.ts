import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { envReplace } from '@pnpm/config.env-replace'
import { readIniFileSync } from 'read-ini-file'

import { isNpmrcReadableKey } from './localConfig.js'

export interface NpmrcConfigResult {
  /**
   * Merged auth/registry config from all sources.
   * Priority (lowest to highest): builtin < defaults < user < auth.ini < workspace < CLI
   */
  mergedConfig: Record<string, unknown>
  /** Raw config suitable for pnpmConfig.authConfig (filtered through pickIniConfig by consumer) */
  rawConfig: Record<string, unknown>
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
    env
  )

  // Read user .npmrc (from npmrcAuthFile setting or ~/.npmrc)
  const userConfig = readAndFilterNpmrc(userConfigPath, warnings, env)

  // Read pnpm auth file (~/.config/pnpm/auth.ini)
  const pnpmAuthConfig = readAndFilterNpmrc(
    path.join(opts.configDir, 'auth.ini'),
    warnings,
    env
  )

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
    opts.cliOptions,
    workspaceNpmrc,
    pnpmAuthConfig,
    userConfig,
    opts.defaultOptions,
  ])

  // Merge all sources (lowest to highest priority):
  // builtin < defaults < user < auth.ini < workspace < CLI
  const mergedConfig: Record<string, unknown> = {}
  for (const source of [pnpmBuiltinConfig, opts.defaultOptions, userConfig, pnpmAuthConfig, workspaceNpmrc, opts.cliOptions]) {
    for (const [key, value] of Object.entries(source)) {
      if (isNpmrcReadableKey(key)) {
        mergedConfig[key] = value
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
    ...opts.cliOptions,
  }

  return {
    mergedConfig,
    rawConfig,
    workspaceNpmrc,
    userConfig,
    localPrefix,
    warnings,
  }
}

function readAndFilterNpmrc (
  filePath: string,
  warnings: string[],
  env: Record<string, string | undefined>
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

  const result: Record<string, unknown> = {}
  for (const [rawKey, rawValue] of Object.entries(raw)) {
    // Apply ${VAR} substitution to both keys and values
    const key = substituteEnv(rawKey, env, warnings)
    const value = typeof rawValue === 'string'
      ? substituteEnv(rawValue, env, warnings)
      : rawValue

    // Only keep auth/registry related keys
    if (isNpmrcReadableKey(key)) {
      result[key] = value
    }
  }
  return result
}

function substituteEnv (value: string, env: Record<string, string | undefined>, warnings: string[]): string {
  try {
    return envReplace(value, env)
  } catch (err) {
    warnings.push(err instanceof Error ? err.message : String(err))
    return value
  }
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
