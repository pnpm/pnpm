import kebabCase from 'lodash.kebabcase'
import { globalWarn } from '@pnpm/logger'
import { runNpm } from '@pnpm/run-npm'
import { type ConfigCommandOptions } from './ConfigCommandOptions'
import { settingShouldFallBackToNpm } from './configSet'

export function configGet (opts: ConfigCommandOptions, key: string): { output: string, exitCode: number } {
  if (opts.global && settingShouldFallBackToNpm(key)) {
    const { status: exitCode } = runNpm(opts.npmPath, ['config', 'get', key])
    return { output: '', exitCode: exitCode ?? 0 }
  }
  const config = opts.rawConfig[kebabCase(key)]
  const output = displayConfig(config, opts)
  return { output, exitCode: 0 }
}

type DisplayConfigOptions = Pick<ConfigCommandOptions, 'json'>

function displayConfig (config: unknown, opts: DisplayConfigOptions): string {
  if (opts.json) return JSON.stringify(config, undefined, 2)
  if (Array.isArray(config)) {
    globalWarn('`pnpm config get` would display an array as comma-separated list due to legacy implementation, use `--json` to print them as json')
    return config.join(',')
  }
  if (typeof config === 'object') {
    return JSON.stringify(config, undefined, 2)
  }
  return String(config)
}
