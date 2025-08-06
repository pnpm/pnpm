import kebabCase from 'lodash.kebabcase'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { ParseErrorBase, getObjectValueByPropertyPath, parsePropertyPath } from '@pnpm/object.property-path'
import { runNpm } from '@pnpm/run-npm'
import { type ConfigCommandOptions } from './ConfigCommandOptions'
import { settingShouldFallBackToNpm } from './configSet'

export function configGet (opts: ConfigCommandOptions, key: string): { output: string, exitCode: number } {
  if (opts.global && settingShouldFallBackToNpm(key)) {
    const { status: exitCode } = runNpm(opts.npmPath, ['config', 'get', key])
    return { output: '', exitCode: exitCode ?? 0 }
  }
  // .npmrc has some weird config key that cannot be parsed as property path (such as auth token and registries),
  // so it would be tried first then fall back to parsing property paths
  const config = opts.rawConfig[kebabCase(key)] ?? getConfigByPropertyPath(opts.rawConfig, key)
  const output = displayConfig(config, opts)
  return { output, exitCode: 0 }
}

function getConfigByPropertyPath (rawConfig: Record<string, unknown>, propertyPath: string): unknown {
  let topLevelKey: string | number
  let suffix: Iterable<string | number>
  try {
    ; [topLevelKey, ...suffix] = parsePropertyPath(propertyPath)
  } catch (error) {
    if (error instanceof ParseErrorBase) {
      globalWarn(error.message)
      return undefined
    }
    throw error
  }
  if (topLevelKey == null || topLevelKey === '') {
    throw new PnpmError('NO_CONFIG_KEY', 'Cannot get config with an empty key')
  }
  const kebabKey = kebabCase(String(topLevelKey))
  return getObjectValueByPropertyPath(rawConfig[kebabKey], suffix)
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
