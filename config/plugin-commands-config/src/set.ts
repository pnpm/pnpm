import * as configCmd from './config.js'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'

export const rcOptionsTypes = configCmd.rcOptionsTypes
export const cliOptionsTypes = configCmd.cliOptionsTypes
export const help = configCmd.help

export const commandNames = ['set']

export async function handler (opts: ConfigCommandOptions, params: string[]): Promise<configCmd.ConfigHandlerResult> {
  return configCmd.handler(opts, ['set', ...params])
}
