import { join } from 'node:path'

import type { Config } from './Config.js'

const REGEX = /^~[/\\]/

export const transformPath = (path: string, homedir: string): string =>
  REGEX.test(path) ? join(homedir, path.replace(REGEX, '')) : path

const GLOBAL_DIR_KEYS = [
  'globalBinDir',
  'globalDir',
] as const satisfies Array<keyof Config>

const PATH_KEYS = [
  'cacheDir',
  ...GLOBAL_DIR_KEYS,
  'pnpmHomeDir',
  'storeDir',
] as const satisfies Array<keyof Config>

type PathKey = typeof PATH_KEYS[number]

type PathConfig = Partial<Pick<Config, PathKey>>

/**
 * Expand a leading `~/` in the two settings `globalPkgDir` and `bin` are
 * derived from, before that derivation runs — a tilde left in place would
 * become a directory literally named `~`. The `transformPathKeys` pass at
 * the end of `getConfig` reaches these two again, by then a no-op.
 */
export function transformGlobalDirKeys (config: PathConfig, homedir: string): void {
  transformKeys(config, homedir, GLOBAL_DIR_KEYS)
}

export function transformPathKeys (config: PathConfig, homedir: string): void {
  transformKeys(config, homedir, PATH_KEYS)
}

function transformKeys (config: PathConfig, homedir: string, keys: readonly PathKey[]): void {
  for (const key of keys) {
    if (config[key]) {
      config[key] = transformPath(config[key], homedir)
    }
  }
}
