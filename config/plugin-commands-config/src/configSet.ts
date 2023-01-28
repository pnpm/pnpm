import path from 'path'
import { readIniFile } from 'read-ini-file'
import { writeIniFile } from 'write-ini-file'
import { ConfigCommandOptions } from './ConfigCommandOptions'

export async function configSet (opts: ConfigCommandOptions, key: string, value: string | null) {
  const configPath = opts.global ? path.join(opts.configDir, 'rc') : path.join(opts.dir, '.npmrc')
  const settings = await safeReadIniFile(configPath)
  if (value == null) {
    if (settings[key] == null) return
    delete settings[key]
  } else {
    settings[key] = value
  }
  await writeIniFile(configPath, settings)
}

async function safeReadIniFile (configPath: string): Promise<Record<string, unknown>> {
  try {
    return await readIniFile(configPath) as Record<string, unknown>
  } catch (err: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
    if (err.code === 'ENOENT') return {}
    throw err
  }
}
