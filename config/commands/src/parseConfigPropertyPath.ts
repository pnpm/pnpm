import { types } from '@pnpm/config.reader'
import { parsePropertyPath } from '@pnpm/object.property-path'
import camelcase from 'camelcase'
import kebabCase from 'lodash.kebabcase'

/**
 * Just like {@link parsePropertyPath} but the first element may be converted into camelCase
 * if it's a known typed config key, to match the camelCase keys produced by {@link configToRecord}.
 */
export function * parseConfigPropertyPath (propertyPath: string): Generator<string | number, void, void> {
  const iter = parsePropertyPath(propertyPath)

  const first = iter.next()
  if (first.done) return
  yield normalizeTopLevelConfigName(first.value)

  yield * iter
}

/**
 * Turn a top-level config name into camelCase if it's a known typed config key.
 * Otherwise, return the string as-is.
 */
function normalizeTopLevelConfigName (configName: string | number): string {
  if (typeof configName === 'number') return configName.toString()

  const kebabKey = kebabCase(configName)
  if (Object.hasOwn(types, kebabKey)) return camelcase(kebabKey, { locale: 'en-US' })

  return configName
}
