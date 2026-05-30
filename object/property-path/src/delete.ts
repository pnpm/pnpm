import { parsePropertyPath } from './parse.js'
import { rejectUnsafeKeys } from './unsafeKeys.js'

type ObjectOrArray = Record<string | number, unknown> | unknown[]

/**
 * Remove the value at a nested property path on {@link object}.
 *
 * No-op if the path does not resolve to an existing value. Array elements are
 * removed via `splice` so no `null` hole is left behind.
 *
 * Throws on unsafe keys (`__proto__`, `constructor`, `prototype`) to prevent
 * prototype pollution.
 */
export function deleteObjectValueByPropertyPath (object: ObjectOrArray, propertyPath: Iterable<string | number>): void {
  const path = Array.from(propertyPath)
  if (path.length === 0) return
  rejectUnsafeKeys(path)

  let obj: ObjectOrArray = object
  for (let i = 0; i < path.length - 1; i++) {
    const key = path[i]
    if (
      typeof obj !== 'object' ||
      obj === null ||
      !Object.hasOwn(obj, key) ||
      (Array.isArray(obj) && typeof key !== 'number')
    ) {
      return
    }
    obj = (obj as Record<string | number, unknown>)[key] as ObjectOrArray
  }

  if (typeof obj !== 'object' || obj === null) return

  const lastKey = path[path.length - 1]
  if (Array.isArray(obj) && isArrayIndex(lastKey)) {
    obj.splice(Number(lastKey), 1)
    return
  }
  delete (obj as Record<string | number, unknown>)[lastKey]
}

export const deleteObjectValueByPropertyPathString = (object: ObjectOrArray, propertyPath: string): void =>
  deleteObjectValueByPropertyPath(object, parsePropertyPath(propertyPath))

function isArrayIndex (key: string | number): boolean {
  if (typeof key === 'number') return Number.isInteger(key) && key >= 0
  if (!/^(?:0|[1-9]\d*)$/.test(key)) return false
  return Number.isSafeInteger(Number(key))
}
