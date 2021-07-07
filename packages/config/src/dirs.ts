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
  if (opts.platform !== 'win32' && opts.platform !== 'darwin') {
    return path.join(os.homedir(), '.local/state/pnpm')
  }
  if (opts.env.LOCALAPPDATA) {
    return path.join(opts.env.LOCALAPPDATA, 'pnpm-state')
  }
  return path.join(os.homedir(), '.pnpm-state')
}
