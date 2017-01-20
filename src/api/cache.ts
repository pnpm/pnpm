import rimraf = require('rimraf-then')
import expandTilde from '../fs/expandTilde'

export const CACHE_PATH = expandTilde('~/.pnpm-cache')

export function cleanCache (cachePath?: string) {
  return rimraf(expandTilde(cachePath || CACHE_PATH))
}
