import os from 'os'
import path from 'path'

export function getCacheDir (
  opts: {
    env: NodeJS.ProcessEnv
    platform: string
  }
) {
  if (opts.env.XDG_CACHE_HOME) {
    return path.join(opts.env.XDG_CACHE_HOME, 'pnpm')
  }
  if (opts.platform === 'darwin') {
    return path.join(os.homedir(), 'Library/Caches/pnpm')
  }
  if (opts.platform !== 'win32') {
    return path.join(os.homedir(), '.cache/pnpm')
  }
  if (opts.env.LOCALAPPDATA) {
    return path.join(opts.env.LOCALAPPDATA, 'pnpm-cache')
  }
  return path.join(os.homedir(), '.pnpm-cache')
}

export function getStateDir (
  opts: {
    env: NodeJS.ProcessEnv
    platform: string
  }
) {
  if (opts.env.XDG_STATE_HOME) {
    return path.join(opts.env.XDG_STATE_HOME, 'pnpm')
  }
  if (opts.platform !== 'win32') {
    return path.join(os.homedir(), '.local/state/pnpm')
  }
  if (opts.env.LOCALAPPDATA) {
    return path.join(opts.env.LOCALAPPDATA, 'pnpm-state')
  }
  return path.join(os.homedir(), '.pnpm-state')
}

export function getDataDir (
  opts: {
    env: NodeJS.ProcessEnv
    platform: string
  }
) {
  if (opts.env.PNPM_HOME) {
    return opts.env.PNPM_HOME
  }
  if (opts.env.XDG_DATA_HOME) {
    return path.join(opts.env.XDG_DATA_HOME, 'pnpm')
  }
  if (opts.platform === 'darwin') {
    return path.join(os.homedir(), 'Library/pnpm')
  }
  if (opts.platform !== 'win32') {
    return path.join(os.homedir(), '.local/share/pnpm')
  }
  if (opts.env.LOCALAPPDATA) {
    return path.join(opts.env.LOCALAPPDATA, 'pnpm')
  }
  return path.join(os.homedir(), '.pnpm')
}

export function getConfigDir (
  opts: {
    env: NodeJS.ProcessEnv
    platform: string
  }
) {
  if (opts.env.XDG_CONFIG_HOME) {
    return path.join(opts.env.XDG_CONFIG_HOME, 'pnpm')
  }
  if (opts.platform === 'darwin') {
    return path.join(os.homedir(), 'Library/Preferences/pnpm')
  }
  if (opts.platform !== 'win32') {
    return path.join(os.homedir(), '.config/pnpm')
  }
  if (opts.env.LOCALAPPDATA) {
    return path.join(opts.env.LOCALAPPDATA, 'pnpm/config')
  }
  return path.join(os.homedir(), '.config/pnpm')
}
