import kebabCase from 'lodash.kebabcase'
import { parsePropertyPath } from '@pnpm/object.property-path'

/**
 * Just like {@link parsePropertyPath} but the first element is converted into kebab-case.
 */
export function * parseConfigPropertyPath (propertyPath: string): Generator<string | number, void, void> {
  const iter = parsePropertyPath(propertyPath)

  const first = iter.next()
  if (first.done) return
  yield typeof first.value === 'string'
    ? kebabCase(first.value)
    : first.value

  yield * iter
}
