import path from 'path'
import { type config } from '../../src/index.js'
import { type ConfigCommandOptions } from '../../src/ConfigCommandOptions.js'

export function getOutputString (result: config.ConfigHandlerResult): string {
  if (result == null) throw new Error('output is null or undefined')
  if (typeof result === 'string') return result
  if (typeof result === 'object') return result.output
  const _typeGuard: never = result // eslint-disable-line @typescript-eslint/no-unused-vars
  throw new Error('unreachable')
}

export const DEFAULT_OPTS: ConfigCommandOptions = {
  dir: process.cwd(),
  cliOptions: {},
  configDir: process.cwd(),
  globalconfig: path.join(process.cwd(), 'rc'),
  global: false,
  npmPath: undefined,
  rawConfig: {},
  workspaceDir: undefined,
}
