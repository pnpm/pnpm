// We use object-hash even though node-object-hash is faster.
// Unlike node-object-hash, object-hash is streaming the hash updates,
// avoiding "Invalid string length" errors.
import hash from 'object-hash'

const defaultOptions: hash.BaseOptions = {
  respectType: false,
  algorithm: 'sha1',
}

const withoutSortingOptions: hash.BaseOptions = {
  ...defaultOptions,
  unorderedArrays: false,
  unorderedObjects: false,
  unorderedSets: false,
}

const withSortingOptions: hash.BaseOptions = {
  ...defaultOptions,
  unorderedArrays: true,
  unorderedObjects: true,
  unorderedSets: true,
}

function hashUnknown (object: unknown, options: hash.BaseOptions) {
  if (object === undefined) {
    // '0'.repeat(40) to match the length of other returned sha1 hashes.
    return '0000000000000000000000000000000000000000'
  }
  return hash(object, options)
}

export const hashObjectWithoutSorting = (object: unknown) => hashUnknown(object, withoutSortingOptions)
export const hashObject = (object: unknown) => hashUnknown(object, withSortingOptions)
