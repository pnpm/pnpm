import kebabCase from 'lodash.kebabcase'
import camelcase from 'camelcase'

const PREFIX = 'pnpm_config_'

/**
 * Represent the config object of `@pnpm/npm-conf`.
 *
 * This type was defined here because `@pnpm/npm-conf` has no typescript definition at the time of writing.
 */
export interface NpmConf {
  get: (key: string) => unknown
  set: (key: string, value: string | string[]) => void
}

/**
 * Pair of a camelCase key and a parsed value
 */
export interface ConfigPair<Value> {
  key: string
  value: Value
}

/**
 * Load all the environment variables whose names start with {@link PREFIX} into a config object to parse their raw values according
 * to the types then emit back pairs of camelCase keys and parsed values.
 */
export function * parseEnvVars (npmConf: NpmConf, env: NodeJS.ProcessEnv): Generator<ConfigPair<unknown>, void, void> {
  for (const envKey in env) {
    const suffix = getEnvKeySuffix(envKey)
    if (!suffix) continue
    const envValue = env[envKey]
    if (envValue == null) continue
    const confKey = kebabCase(suffix)
    const confValue = parseEnvVar(suffix, envValue)
    npmConf.set(confKey, confValue)
    const key = camelcase(suffix)
    const value = npmConf.get(confKey)
    yield { key, value }
  }
}

/**
 * Return the suffix if {@link envKey} starts with {@link PREFIX} and is fully lower_snake_case.
 * Otherwise, return `undefined`.
 */
function getEnvKeySuffix (envKey: string): string | undefined {
  if (!envKey.startsWith(PREFIX)) return undefined
  const suffix = envKey.slice(PREFIX.length)
  if (!isEnvKeySuffix(suffix)) return undefined
  return suffix
}

/**
 * A valid env key suffix is lower_snake_case without redundant underscore characters.
 */
function isEnvKeySuffix (envKeySuffix: string): boolean {
  return envKeySuffix.split('_').every(segment => /^[a-z0-9]+$/.test(segment))
}

function parseEnvVar (envKeySuffix: string, envValue: string): string | string[] {
  switch (envKeySuffix) {
  case 'hoist_pattern':
  case 'public_hoist_pattern':
    return parseEnvVarAsList(envValue)
  }
  return envValue
}

function parseEnvVarAsList (envValue: string): string[] {
  const npmConfigSep = '\n\n'
  const configSep = envValue.includes(npmConfigSep) ? npmConfigSep : ','
  return envValue.split(configSep)
}
