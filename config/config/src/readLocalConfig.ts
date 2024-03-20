import path from 'node:path'

import { readIniFile } from 'read-ini-file'
import camelcaseKeys from 'camelcase-keys'

import { envReplace } from '@pnpm/config.env-replace'

export async function readLocalConfig(prefix: string): Promise<Record<string, string> & {
  hoist?: boolean | undefined;
}> {
  try {
    const ini = (await readIniFile(path.join(prefix, '.npmrc'))) as Record<
      string,
      string
    >

    const config: Record<string, string> & {
      hoist?: boolean
    } = camelcaseKeys(ini)

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
    // @ts-ignore
    if (err.code !== 'ENOENT') {
      throw err
    }

    return {}
  }
}
