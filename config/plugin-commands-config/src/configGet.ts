import kebabCase from 'lodash.kebabcase'
import { runNpm } from '@pnpm/run-npm'
import { type ConfigCommandOptions } from './ConfigCommandOptions'
import { settingShouldFallBackToNpm } from './configSet'

export function configGet (opts: ConfigCommandOptions, key: string): { output: string, exitCode: number } {
  if (opts.global && settingShouldFallBackToNpm(key)) {
    const { status: exitCode } = runNpm(opts.npmPath, ['config', 'get', key])
    return { output: '', exitCode: exitCode ?? 0 }
  }
  const config = opts.rawConfig[kebabCase(key)]
  return { output: Array.isArray(config) ? config.join(',') : String(config), exitCode: 0 }
}
