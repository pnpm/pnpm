import { PnpmError } from '@pnpm/error'

const UNSAFE_KEYS = new Set(['__proto__', 'constructor', 'prototype'])

export class UnsafePropertyPathKeyError extends PnpmError {
  readonly key: string
  constructor (key: string) {
    super('UNSAFE_PROPERTY_PATH_KEY', `Key "${key}" is not allowed in a property path`)
    this.key = key
  }
}

/**
 * Throw if the property path contains a key that could trigger prototype
 * pollution when used to mutate an object (e.g. via {@link setObjectValueByPropertyPath}).
 */
export function rejectUnsafeKeys (propertyPath: Iterable<string | number>): void {
  for (const segment of propertyPath) {
    if (typeof segment === 'string' && UNSAFE_KEYS.has(segment)) {
      throw new UnsafePropertyPathKeyError(segment)
    }
  }
}
