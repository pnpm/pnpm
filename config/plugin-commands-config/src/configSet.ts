import path from 'path'
import readIniFile from 'read-ini-file'
import writeIniFile from 'write-ini-file'
import { ConfigCommandOptions } from './ConfigCommandOptions'

export async function configSet (opts: ConfigCommandOptions, key: string, value: string) {
  const configPath = opts.global ? path.join(opts.configDir, 'rc') : path.join(opts.dir, '.npmrc')
  const settings = await readIniFile(configPath)
  settings[key] = value
  await writeIniFile(configPath, settings)
}
