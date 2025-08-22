import { encode } from 'ini'
import { runNpm } from '@pnpm/run-npm'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'
import { getRawSetting } from './rawSetting.js'
import { settingShouldFallBackToNpm } from './settingShouldFallBackToNpm.js'

export function configGet (opts: ConfigCommandOptions, key: string): { output: string, exitCode: number } {
  if (opts.global && settingShouldFallBackToNpm(key)) {
    const { status: exitCode } = runNpm(opts.npmPath, ['config', 'get', key])
    return { output: '', exitCode: exitCode ?? 0 }
  }
  const config = getRawSetting(opts, key)
  const output = displayConfig(config, opts)
  return { output, exitCode: 0 }
}

type DisplayConfigOptions = Pick<ConfigCommandOptions, 'json'>

function displayConfig (config: unknown, opts: DisplayConfigOptions): string {
  if (Boolean(opts.json) || Array.isArray(config)) {
    return JSON.stringify(config, undefined, 2)
  }
  if (typeof config === 'object' && config != null) {
    return encode(config)
  }
  return String(config)
}
