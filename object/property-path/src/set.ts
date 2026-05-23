import { PnpmError } from '@pnpm/error'

import { parsePropertyPath } from './parse.js'
import { rejectUnsafeKeys } from './unsafe-keys.js'

type ObjectOrArray = Record<string | number, unknown> | unknown[]

export class EmptyPropertyPathError extends PnpmError {
  constructor () {
    super('EMPTY_PROPERTY_PATH', 'Cannot set a value with an empty property path')
  }
}

/**
 * Set the value at a nested property path on {@link object}.
 *
 * Creates intermediate objects or arrays as needed. If an intermediate node is
 * a scalar, it is replaced with a new container (array when the next segment is
 * numeric, object otherwise) before descending.
 *
 * Throws on unsafe keys (`__proto__`, `constructor`, `prototype`) to prevent
 * prototype pollution.
 */
export function setObjectValueByPropertyPath (object: ObjectOrArray, propertyPath: Iterable<string | number>, value: unknown): void {
  const path = Array.from(propertyPath)
  if (path.length === 0) throw new EmptyPropertyPathError()
  rejectUnsafeKeys(path)

  let obj: ObjectOrArray = object
  for (let i = 0; i < path.length - 1; i++) {
    const key = path[i]
    const current = (obj as Record<string | number, unknown>)[key]
    if (typeof current !== 'object' || current === null) {
      const replacement: ObjectOrArray = typeof path[i + 1] === 'number' ? [] : {}
      ;(obj as Record<string | number, unknown>)[key] = replacement
      obj = replacement
    } else {
      obj = current as ObjectOrArray
    }
  }

  const lastKey = path[path.length - 1]
  ;(obj as Record<string | number, unknown>)[lastKey] = value
}

export const setObjectValueByPropertyPathString = (object: ObjectOrArray, propertyPath: string, value: unknown): void =>
  setObjectValueByPropertyPath(object, parsePropertyPath(propertyPath), value)
