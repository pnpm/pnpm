import kebabCase from 'lodash.kebabcase'
import { encode } from 'ini'
import { getObjectValueByPropertyPath } from '@pnpm/object.property-path'
import { runNpm } from '@pnpm/run-npm'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'
import { isStrictlyKebabCase } from './isStrictlyKebabCase.js'
import { parseConfigPropertyPath } from './parseConfigPropertyPath.js'
import { settingShouldFallBackToNpm } from './settingShouldFallBackToNpm.js'

export function configGet (opts: ConfigCommandOptions, key: string): { output: string, exitCode: number } {
  if (opts.global && settingShouldFallBackToNpm(key)) {
    const { status: exitCode } = runNpm(opts.npmPath, ['config', 'get', key])
    return { output: '', exitCode: exitCode ?? 0 }
  }
  const config = isStrictlyKebabCase(key)
    ? opts.rawConfig[kebabCase(key)] // we don't parse kebab-case keys as property paths because it's not a valid JS syntax
    : getConfigByPropertyPath(opts.rawConfig, key)
  const output = displayConfig(config, opts)
  return { output, exitCode: 0 }
}

function getConfigByPropertyPath (rawConfig: Record<string, unknown>, propertyPath: string): unknown {
  return getObjectValueByPropertyPath(rawConfig, parseConfigPropertyPath(propertyPath))
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
