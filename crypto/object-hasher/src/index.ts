import isEmpty from 'ramda/src/isEmpty'

// We use object-hash even though node-object-hash is faster.
// Unlike node-object-hash, object-hash is streaming the hash updates,
// avoiding "Invalid string length" errors.
import hash from 'object-hash'

const defaultOptions: hash.NormalOption = {
  respectType: false,
  algorithm: 'sha256',
  encoding: 'base64',
}

const withoutSortingOptions: hash.NormalOption = {
  ...defaultOptions,
  unorderedArrays: false,
  unorderedObjects: false,
  unorderedSets: false,
}

const withSortingOptions: hash.NormalOption = {
  ...defaultOptions,
  unorderedArrays: true,
  unorderedObjects: true,
  unorderedSets: true,
}

function hashUnknown (object: unknown, options: hash.BaseOptions): string {
  if (object === undefined) {
    // '0'.repeat(44) to match the length of other returned sha1 hashes.
    return '00000000000000000000000000000000000000000000'
  }
  return hash(object, options)
}

export const hashObjectWithoutSorting = (object: unknown): string => hashUnknown(object, withoutSortingOptions)
export const hashObject = (object: unknown): string => hashUnknown(object, withSortingOptions)

export type PrefixedHash = `sha256-${string}`
export function hashObjectNullableWithPrefix (object: Record<string, unknown> | undefined): PrefixedHash | undefined {
  if (!object || isEmpty(object)) return undefined
  const packageExtensionsChecksum = hash(object, withSortingOptions)
  return `sha256-${packageExtensionsChecksum}`
}
