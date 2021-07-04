import os from 'os'
import path from 'path'

export default function getCacheDir (
  opts: {
    env: NodeJS.ProcessEnv
    platform: string
  }
) {
  if (opts.env.XDG_CACHE_HOME) {
    return path.join(opts.env.XDG_CACHE_HOME, 'pnpm')
  }
  if (opts.platform !== 'win32') {
    return path.join(os.homedir(), '.cache/pnpm')
  }
  if (opts.env.LOCALAPPDATA) {
    return path.join(opts.env.LOCALAPPDATA, 'pnpm-cache')
  }
  return path.join(os.homedir(), '.pnpm-cache')
}
