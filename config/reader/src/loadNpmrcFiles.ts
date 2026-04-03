import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { envReplace } from '@pnpm/config.env-replace'
import { readIniFileSync } from 'read-ini-file'

import { isIniConfigKey } from './auth.js'

export interface NpmrcConfigResult {
  /** Project .npmrc data (filtered to auth/registry keys) */
  projectConfig: Record<string, unknown>
  /** Workspace .npmrc data (filtered to auth/registry keys), if workspace differs from project */
  workspaceConfig: Record<string, unknown> | undefined
  /** User ~/.npmrc data (filtered to auth/registry keys) */
  userConfig: Record<string, unknown>
  /** pnpm global rc data (filtered to auth/registry keys) */
  pnpmGlobalConfig: Record<string, unknown>
  /** pnpm builtin rc data + inline defaults */
  pnpmBuiltinConfig: Record<string, unknown>
  /**
   * All layers in priority order (highest first), matching the old npmConfig.list layout:
   * [0] = CLI options
   * [1] = npm_config_* env vars (auth/registry only)
   * [2] = (empty — no builtin npm config)
   * [3] = project .npmrc
   * [4] = workspace .npmrc (empty if no workspace or same as project)
   * [5] = user .npmrc
   * [6] = global npmrc
   * [7] = defaults
   */
  layers: Array<Record<string, unknown>>
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
  /** Path to user .npmrc (defaults to ~/.npmrc) */
  userconfig?: string
  /** Path to global npmrc */
  globalconfig?: string
  /** pnpm config directory (for pnpm global rc) */
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

  const userConfigPath = opts.userconfig ?? opts.cliOptions.userconfig as string ?? path.resolve(os.homedir(), '.npmrc')
  const globalConfigPath = opts.globalconfig ?? opts.cliOptions.globalconfig as string ?? undefined

  // Read project .npmrc
  const projectConfig = readAndFilterNpmrc(
    path.resolve(localPrefix, '.npmrc'),
    warnings,
    env
  )

  // Read workspace .npmrc (if different from project)
  let workspaceConfig: Record<string, unknown> | undefined
  if (opts.workspaceDir && opts.workspaceDir !== localPrefix) {
    workspaceConfig = readAndFilterNpmrc(
      path.resolve(opts.workspaceDir, '.npmrc'),
      warnings,
      env
    )
  }

  // Read user ~/.npmrc
  const userConfig = readAndFilterNpmrc(userConfigPath, warnings, env)

  // Read global npmrc
  const globalConfig = globalConfigPath
    ? readAndFilterNpmrc(globalConfigPath, warnings, env)
    : {}

  // Read pnpm global rc
  const pnpmGlobalConfig = readAndFilterNpmrc(
    path.join(opts.configDir, 'rc'),
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

  // Parse npm_config_* env vars for auth/registry keys
  const npmEnvConfig = parseNpmConfigEnvVars(env)

  // Build layers in priority order (highest first), matching old npmConfig.list layout
  const layers: Array<Record<string, unknown>> = [
    opts.cliOptions, // [0] CLI
    npmEnvConfig, // [1] env
    {}, // [2] builtin npm (always empty now)
    projectConfig, // [3] project .npmrc
    workspaceConfig ?? {}, // [4] workspace .npmrc
    userConfig, // [5] user .npmrc
    globalConfig, // [6] global npmrc
    opts.defaultOptions, // [7] defaults
  ]

  // Handle cafile: read and set ca if cafile is configured
  loadCAFile(layers)

  return {
    projectConfig,
    workspaceConfig,
    userConfig,
    pnpmGlobalConfig,
    pnpmBuiltinConfig,
    layers,
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
    if (isIniConfigKey(key)) {
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

function isErrorWithCode (err: unknown, code: string): boolean {
  return err != null && typeof err === 'object' && 'code' in err && err.code === code
}

/**
 * Parse npm_config_* environment variables, keeping only auth/registry keys.
 * Converts env key format: npm_config_foo_bar → foo-bar
 * Special case: npm_config__authtoken → _authToken // cspell:disable-line
 */
function parseNpmConfigEnvVars (env: Record<string, string | undefined>): Record<string, string> {
  const result: Record<string, string> = {}
  for (const [key, value] of Object.entries(env)) {
    if (!/^npm_config_/i.test(key) || !value) continue
    const configKey = envKeyToSetting(key.slice(11))
    if (isIniConfigKey(configKey)) {
      result[configKey] = value
    }
  }
  return result
}

/**
 * Convert an npm_config_ env var suffix to a setting name.
 * Ported from @pnpm/npm-conf/lib/envKeyToSetting.js
 */
function envKeyToSetting (x: string): string {
  const colonIndex = x.indexOf(':')
  if (colonIndex === -1) {
    return normalizeEnvKey(x)
  }
  const firstPart = x.slice(0, colonIndex)
  const secondPart = x.slice(colonIndex + 1)
  return `${normalizeEnvKey(firstPart)}:${normalizeEnvKey(secondPart)}`
}

function normalizeEnvKey (s: string): string {
  s = s.toLowerCase()
  if (s === '_authtoken') return '_authToken' // cspell:disable-line
  let r = s[0]
  for (let i = 1; i < s.length; i++) {
    r += s[i] === '_' ? '-' : s[i]
  }
  return r
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
