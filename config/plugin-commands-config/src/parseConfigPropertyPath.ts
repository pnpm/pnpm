import kebabCase from 'lodash.kebabcase'
import { types } from '@pnpm/config'
import { parsePropertyPath } from '@pnpm/object.property-path'

/**
 * Just like {@link parsePropertyPath} but the first element may be converted into kebab-case
 * if it's part of {@link types}.
 */
export function * parseConfigPropertyPath (propertyPath: string): Generator<string | number, void, void> {
  const iter = parsePropertyPath(propertyPath)

  const first = iter.next()
  if (first.done) return
  yield normalizeTopLevelConfigName(first.value)

  yield * iter
}

/**
 * Turn a top-level config name into kebab-case if it's part of {@link types}.
 * Otherwise, return the string as-is.
 */
function normalizeTopLevelConfigName (configName: string | number): string {
  if (typeof configName === 'number') return configName.toString()

  const kebabKey = kebabCase(configName)
  if (kebabKey in types) return kebabKey

  return configName
}
