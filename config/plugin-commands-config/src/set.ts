import * as configCmd from './config'
import { type ConfigCommandOptions } from './ConfigCommandOptions'

export const rcOptionsTypes = configCmd.rcOptionsTypes
export const cliOptionsTypes = configCmd.cliOptionsTypes
export const help = configCmd.help

export const commandNames = ['set']

export async function handler (opts: ConfigCommandOptions, params: string[]) {
  return configCmd.handler(opts, ['set', ...params])
}
