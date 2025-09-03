import { parsePropertyPath } from './parse.js'

/**
 * Get the value of a property path in a nested object.
 *
 * This function returns `undefined` if it meets non-object at some point.
 */
export function getObjectValueByPropertyPath (object: unknown, propertyPath: Iterable<string | number>): unknown {
  for (const name of propertyPath) {
    if (
      typeof object !== 'object' ||
      object == null ||
      !Object.hasOwn(object, name) ||
      (Array.isArray(object) && typeof name !== 'number')
    ) return undefined

    object = (object as Record<string | number, unknown>)[name]
  }

  return object
}

/**
 * Get the value of a property path in a nested object.
 *
 * This function returns `undefined` if it meets non-object at some point.
 */
export const getObjectValueByPropertyPathString =
  (object: unknown, propertyPath: string): unknown => getObjectValueByPropertyPath(object, parsePropertyPath(propertyPath))
