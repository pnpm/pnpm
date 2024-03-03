import path from 'path'
import camelcaseKeys from 'camelcase-keys'
import { envReplace } from '@pnpm/config.env-replace'
import { readIniFile } from 'read-ini-file'

export async function readLocalConfig (prefix: string) {
  try {
    const ini = await readIniFile(path.join(prefix, '.npmrc')) as Record<string, string>
    const config = camelcaseKeys(ini) as (Record<string, string> & { hoist?: boolean })
    if (config.shamefullyFlatten) {
      config.hoistPattern = '*'
      // TODO: print a warning
    }
    if (config.hoist === false) {
      config.hoistPattern = ''
    }
    for (const [key, val] of Object.entries(config)) {
      if (typeof val === 'string') {
        try {
          config[key] = envReplace(val, process.env)
        } catch (err) {
          // ignore
        }
      }
    }
    return config
  } catch (err: unknown) {
    if (typeof err === 'object' && err && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
}
