import isEmpty from 'ramda/src/isEmpty'

export function removeUndefinedFromObject<Object> (object: Object): Object | undefined {
  if (typeof object !== 'object' || object === null) return object
  if (Array.isArray(object)) {
    return object.map(removeUndefinedFromObject) as Object
  }
  const result = {} as Object
  for (const key in object) {
    const value = object[key]
    if (value !== undefined) {
      const newValue = removeUndefinedFromObject(value)
      if (newValue !== undefined) {
        result[key] = newValue
      }
    }
  }
  return isEmpty(result) ? undefined : result
}
