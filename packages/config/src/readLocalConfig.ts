import path from 'path'
import camelcaseKeys from 'camelcase-keys'
import envReplace from '@npmcli/config/lib/env-replace.js'
import readIniFile from 'read-ini-file'

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
        config[key] = envReplace(val, process.env)
      }
    }
    return config
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') throw err
    return {}
  }
}
