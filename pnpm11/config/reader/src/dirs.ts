import os from 'node:os'
import path from 'node:path'

import { GLOBAL_CONFIG_YAML_FILENAME } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'

export function getGlobalConfigPath (configDir: string): string {
  return path.join(configDir, GLOBAL_CONFIG_YAML_FILENAME)
}

export function getCacheDir (
  opts: {
    env: NodeJS.ProcessEnv
    platform: string
  }
): string {
  const xdgCacheHome = pathFromEnv('XDG_CACHE_HOME', opts)
  if (xdgCacheHome) {
    return path.join(xdgCacheHome, 'pnpm')
  }
  if (opts.platform === 'darwin') {
    return path.join(os.homedir(), 'Library/Caches/pnpm')
  }
  if (opts.platform !== 'win32') {
    return path.join(os.homedir(), '.cache/pnpm')
  }
  const localAppData = pathFromEnv('LOCALAPPDATA', opts)
  if (localAppData) {
    return path.join(localAppData, 'pnpm-cache')
  }
  return path.join(os.homedir(), '.pnpm-cache')
}

export function getStateDir (
  opts: {
    env: NodeJS.ProcessEnv
    platform: string
  }
): string {
  const xdgStateHome = pathFromEnv('XDG_STATE_HOME', opts)
  if (xdgStateHome) {
    return path.join(xdgStateHome, 'pnpm')
  }
  if (opts.platform !== 'win32') {
    return path.join(os.homedir(), '.local/state/pnpm')
  }
  const localAppData = pathFromEnv('LOCALAPPDATA', opts)
  if (localAppData) {
    return path.join(localAppData, 'pnpm-state')
  }
  return path.join(os.homedir(), '.pnpm-state')
}

export function getDataDir (
  opts: {
    env: NodeJS.ProcessEnv
    platform: string
  }
): string {
  const pnpmHome = pathFromEnv('PNPM_HOME', opts)
  if (pnpmHome) {
    return pnpmHome
  }
  const xdgDataHome = pathFromEnv('XDG_DATA_HOME', opts)
  if (xdgDataHome) {
    return path.join(xdgDataHome, 'pnpm')
  }
  if (opts.platform === 'darwin') {
    return path.join(os.homedir(), 'Library/pnpm')
  }
  if (opts.platform !== 'win32') {
    return path.join(os.homedir(), '.local/share/pnpm')
  }
  const localAppData = pathFromEnv('LOCALAPPDATA', opts)
  if (localAppData) {
    return path.join(localAppData, 'pnpm')
  }
  return path.join(os.homedir(), '.pnpm')
}

export function getConfigDir (
  opts: {
    env: NodeJS.ProcessEnv
    platform: string
  }
): string {
  const xdgConfigHome = pathFromEnv('XDG_CONFIG_HOME', opts)
  if (xdgConfigHome) {
    return path.join(xdgConfigHome, 'pnpm')
  }
  if (opts.platform === 'darwin') {
    return path.join(os.homedir(), 'Library/Preferences/pnpm')
  }
  if (opts.platform !== 'win32') {
    return path.join(os.homedir(), '.config/pnpm')
  }
  const localAppData = pathFromEnv('LOCALAPPDATA', opts)
  if (localAppData) {
    return path.join(localAppData, 'pnpm/config')
  }
  return path.join(os.homedir(), '.config/pnpm')
}

// Windows expands `%VAR%` for REG_EXPAND_SZ values before a process starts.
// A REG_SZ value, or a `set` that ran while the referenced variable was
// unset, still contains the reference. An unexpanded relative value would
// be created as a directory under the current working directory.
function pathFromEnv (
  name: string,
  opts: {
    env: NodeJS.ProcessEnv
    platform: string
  }
): string | undefined {
  const value = opts.env[name]
  if (!value) return undefined
  return expandWindowsDirEnv(value, {
    platform: opts.platform,
    name,
    env: opts.env,
  })
}

function expandWindowsDirEnv (
  value: string,
  opts: {
    platform: string
    name: string
    env: NodeJS.ProcessEnv
  }
): string {
  if (opts.platform !== 'win32') return value
  let current = value
  const seen = new Set<string>()
  while (firstPercentVar(current) != null) {
    if (seen.has(current) || seen.size === 32) break
    seen.add(current)
    const step = substituteOnce(current, varName => lookupEnv(opts.env, varName))
    if (!step.changed) break
    current = step.value
  }
  const reference = firstPercentVar(current)
  if (reference != null) {
    throw new PnpmError(
      'UNEXPANDED_ENV_IN_PATH',
      `${opts.name} contains an unexpanded environment variable: ${reference}`,
      { hint: 'Set the referenced variable, or remove the %VAR% reference from this path.' }
    )
  }
  return current
}

function substituteOnce (
  value: string,
  lookup: (name: string) => string | undefined
): { value: string, changed: boolean } {
  let next = ''
  let changed = false
  let rest = value
  while (rest.includes('%')) {
    const start = rest.indexOf('%')
    next += rest.slice(0, start)
    const end = rest.indexOf('%', start + 1)
    if (end === -1) {
      next += rest.slice(start)
      return { value: next, changed }
    }
    const name = rest.slice(start + 1, end)
    if (!isWindowsEnvName(name)) {
      next += '%'
      rest = rest.slice(start + 1)
      continue
    }
    const replacement = lookup(name)
    if (replacement == null) {
      next += `%${name}%`
      rest = rest.slice(end + 1)
      continue
    }
    next += replacement
    changed = true
    rest = rest.slice(end + 1)
  }
  next += rest
  return { value: next, changed }
}

function firstPercentVar (value: string): string | undefined {
  let rest = value
  while (rest.includes('%')) {
    const start = rest.indexOf('%')
    const end = rest.indexOf('%', start + 1)
    if (end === -1) return undefined
    const name = rest.slice(start + 1, end)
    if (isWindowsEnvName(name)) return `%${name}%`
    rest = rest.slice(start + 1)
  }
  return undefined
}

function isWindowsEnvName (name: string): boolean {
  if (name.length === 0) return false
  for (const char of name) {
    const isNameChar = (char >= 'A' && char <= 'Z') ||
      (char >= 'a' && char <= 'z') ||
      (char >= '0' && char <= '9') ||
      char === '_' ||
      char === '.' ||
      char === '(' ||
      char === ')'
    if (!isNameChar) return false
  }
  return true
}

function lookupEnv (env: NodeJS.ProcessEnv, name: string): string | undefined {
  if (Object.hasOwn(env, name)) return env[name]
  const upper = name.toUpperCase()
  for (const key of Object.keys(env)) {
    if (key.toUpperCase() === upper) return env[key]
  }
  return undefined
}
