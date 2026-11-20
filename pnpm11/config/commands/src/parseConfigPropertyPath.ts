import { parsePropertyPath } from '@pnpm/object.property-path'
import camelcase from 'camelcase'

/**
 * Just like {@link parsePropertyPath} but the first element is converted to camelCase
 * to match the camelCase keys produced by {@link configToRecord}.
 */
export function * parseConfigPropertyPath (propertyPath: string): Generator<string | number, void, void> {
  const iter = parsePropertyPath(propertyPath)

  const first = iter.next()
  if (first.done) return
  yield typeof first.value === 'number' ? first.value : camelcase(first.value, { locale: 'en-US' })

  yield * iter
}
