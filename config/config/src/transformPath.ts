import { join } from 'path'
import { type Config } from './Config'

const REGEX = /^~[/\\]/

export const transformPath = (path: string, homedir: string): string =>
  REGEX.test(path) ? join(homedir, path.replace(REGEX, '')) : path

const PATH_KEYS = [
  'cacheDir',
  'globalBinDir',
  'globalDir',
  'pnpmHomeDir',
  'storeDir',
] as const satisfies Array<keyof Config>

export function transformPathKeys (config: Partial<Pick<Config, typeof PATH_KEYS[number]>>, homedir: string): void {
  for (const key of PATH_KEYS) {
    if (config[key]) {
      config[key] = transformPath(config[key], homedir)
    }
  }
}
