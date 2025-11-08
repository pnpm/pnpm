import path from 'path'
import url from 'url'
import kebabCase from 'lodash.kebabcase'
import camelcase from 'camelcase'

const PREFIX = 'pnpm_config_'

export type ValueConstructor =
  | ArrayConstructor
  | BooleanConstructor
  | NumberConstructor
  | StringConstructor

export type ModuleSchema =
  | typeof path
  | typeof url

export type ValueSchema = ValueConstructor | ModuleSchema

export type LiteralSchema = string | boolean | null

export type UnionVariant = LiteralSchema | ValueSchema

export type Schema = ValueSchema | UnionVariant[]

export type GetSchema = (key: string) => Schema | undefined

/**
 * Pair of a camelCase key and a parsed value
 */
export interface ConfigPair<Value> {
  key: string
  value: Value
}

/**
 * Parse all the environment variables whose names start with {@link PREFIX} according to the {@link types} then emit
 * pairs of camelCase keys and parsed values.
 */
export function * parseEnvVars (getSchema: GetSchema, env: NodeJS.ProcessEnv): Generator<ConfigPair<unknown>, void, void> {
  for (const envKey in env) {
    const suffix = getEnvKeySuffix(envKey)
    if (!suffix) continue
    const envValue = env[envKey]
    if (envValue == null) continue
    const schemaKey = kebabCase(suffix)
    const schema = getSchema(schemaKey)
    if (schema == null) continue
    const key = camelcase(suffix)
    const value = parseValueBySchema(schema, envValue, env as { HOME?: string })
    yield { key, value }
  }
}

function parseValueBySchema (schema: Schema, envVar: string, env: { HOME?: string }): unknown {
  if (Array.isArray(schema)) {
    return parseValueByTypeUnion(schema, envVar, env)
  } else if (typeof schema === 'function') {
    return parseValueByConstructor(schema, envVar)
  } else if (schema && typeof schema === 'object') {
    return parseValueByModule(schema, envVar, env)
  }

  const _typeGuard: never = schema
  throw new Error(`Invalid schema: ${JSON.stringify(_typeGuard)}`)
}

function parseValueByTypeUnion (schema: readonly UnionVariant[], envVar: string, env: { HOME?: string }): unknown {
  for (const variant of sortUnionVariant(schema)) {
    let value: unknown
    switch (typeof variant) {
    case 'string':
      value = parseStringLiteral(variant, envVar)
      break
    case 'boolean':
      value = parseBooleanLiteral(variant, envVar)
      break
    case 'function':
      value = parseValueByConstructor(variant, envVar)
      break
    case 'object':
      value = variant === null
        ? parseNullLiteral(envVar)
        : parseValueByModule(variant, envVar, env)
      break
    default: {
      const _typeGuard: never = variant
      throw new Error(`Invalid schema variant: ${JSON.stringify(_typeGuard)}`)
    }
    }
    if (value !== undefined) return value
  }

  return undefined
}

function parseStringLiteral<StringLiteral extends string> (schema: StringLiteral, envVar: string): StringLiteral | undefined {
  return envVar === schema ? schema : undefined
}

function parseBooleanLiteral<BooleanLiteral extends boolean> (schema: BooleanLiteral, envVar: string): BooleanLiteral | undefined {
  return schema.toString() === envVar ? schema : undefined
}

function parseNullLiteral (envVar: string): null | undefined {
  return envVar === 'null' ? null : undefined
}

function parseValueByConstructor (schema: ValueConstructor, envVar: string): unknown {
  if (schema === Array) {
    const value = tryParseObjectOrArray(envVar)
    return Array.isArray(value) ? value : undefined
  }

  if (schema === Boolean) {
    switch (envVar) {
    case 'true': return true
    case 'false': return false
    default: return undefined
    }
  }

  if (schema === Number) {
    const value = Number(envVar)
    return isNaN(value) ? undefined : value
  }

  if (schema === String) {
    return envVar
  }

  return undefined
}

function parseValueByModule (schema: ModuleSchema, envVar: string, env: { HOME?: string }): unknown {
  if (schema === path) {
    const homePrefix = /^~[/\\]/
    if (env.HOME && homePrefix.test(envVar)) {
      return path.join(env.HOME, envVar.replace(homePrefix, ''))
    }
    return envVar
  }

  if (schema === url) {
    return new url.URL(envVar).toString()
  }

  return undefined
}

/** De-prioritize string parsing to prevent it from shadowing other types */
function sortUnionVariant (variants: readonly UnionVariant[]): UnionVariant[] {
  const sorted = variants.filter(variant => variant !== String)
  if (variants.includes(String)) {
    sorted.push(String)
  }
  return sorted
}

function tryParseObjectOrArray (envVar: string): object | unknown[] | undefined {
  let result: unknown
  try {
    result = JSON.parse(envVar)
  } catch {
    return undefined
  }

  // typeof array is also 'object'
  return result == null || typeof result !== 'object'
    ? undefined
    : result
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
