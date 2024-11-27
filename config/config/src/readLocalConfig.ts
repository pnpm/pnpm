import path from 'path'
import util from 'util'
import camelcaseKeys from 'camelcase-keys'
import { envReplace } from '@pnpm/config.env-replace'
import { readIniFile } from 'read-ini-file'
import { parseField } from '@pnpm/npm-conf/lib/util'
import { types } from './types'

export type LocalConfig = Record<string, string> & { hoist?: boolean }

export async function readLocalConfig (prefix: string): Promise<LocalConfig> {
  try {
    const ini = await readIniFile(path.join(prefix, '.npmrc')) as Record<string, string>
    for (let [key, val] of Object.entries(ini)) {
      if (typeof val === 'string') {
        try {
          key = envReplace(key, process.env)
          ini[key] = parseField(types, envReplace(val, process.env), key) as any // eslint-disable-line
        } catch {}
      }
    }
    const config = camelcaseKeys(ini) as LocalConfig
    if (config.shamefullyFlatten) {
      config.hoistPattern = '*'
      // TODO: print a warning
    }
    if (config.hoist === false) {
      config.hoistPattern = ''
    }
    return config
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
}
