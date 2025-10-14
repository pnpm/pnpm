import path from 'path'
import kebabCase from 'lodash.kebabcase'
import { encode } from 'ini'
import { globalWarn } from '@pnpm/logger'
import { getObjectValueByPropertyPath } from '@pnpm/object.property-path'
import { runNpm } from '@pnpm/run-npm'
import { type ConfigCommandOptions } from './ConfigCommandOptions.js'
import { isStrictlyKebabCase } from './isStrictlyKebabCase.js'
import { parseConfigPropertyPath } from './parseConfigPropertyPath.js'
import { settingShouldFallBackToNpm } from './settingShouldFallBackToNpm.js'

export function configGet (opts: ConfigCommandOptions, key: string): { output: string, exitCode: number } {
  if (key === 'globalconfig') {
    return { output: path.join(opts.configDir, 'rc'), exitCode: 0 }
  }

  const isScopedKey = key.startsWith('@')
  // Exclude scoped keys from npm fallback because they are pnpm-native config
  // that can be read directly from rawConfig (e.g., '@scope:registry')
  if (opts.global && settingShouldFallBackToNpm(key) && !isScopedKey) {
    const { status: exitCode } = runNpm(opts.npmPath, ['config', 'get', key])
    return { output: '', exitCode: exitCode ?? 0 }
  }

  let config: unknown
  if (isStrictlyKebabCase(key)) {
    // we don't parse kebab-case keys as property paths because it's not a valid JS syntax
    config = opts.rawConfig[kebabCase(key)]
  } else if (isScopedKey) {
    // scoped registry keys like '@scope:registry' are used as-is
    config = opts.rawConfig[key]
  } else {
    config = getConfigByPropertyPath(opts.rawConfig, key)
  }

  const output = displayConfig(config, opts)
  return { output, exitCode: 0 }
}

function getConfigByPropertyPath (rawConfig: Record<string, unknown>, propertyPath: string): unknown {
  return getObjectValueByPropertyPath(rawConfig, parseConfigPropertyPath(propertyPath))
}

type DisplayConfigOptions = Pick<ConfigCommandOptions, 'json'>

function displayConfig (config: unknown, opts: DisplayConfigOptions): string {
  if (opts.json) return JSON.stringify(config, undefined, 2)
  if (Array.isArray(config)) {
    globalWarn('`pnpm config get` would display an array as comma-separated list due to legacy implementation, use `--json` to print them as json')
    return config.join(',') // TODO: change this in the next major version
  }
  if (typeof config === 'object' && config != null) {
    return encode(config)
  }
  return String(config)
}
