import { hashObject, hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'

describe('hashObject', () => {
  const hash = hashObject
  it('creates a hash', () => {
    expect(hash({ b: 1, a: 2 })).toEqual('48AVoXIXcTKcnHt8qVKp5vNw4gyOB5VfztHwtYBRcAQ=')
    expect(hash(undefined)).toEqual('00000000000000000000000000000000000000000000')
  })
  it('sorts', () => {
    expect(hash({ b: 1, a: 2 })).toEqual(hash({ a: 2, b: 1 }))
    expect(hash({ b: new Set([1, 2, 3]), a: [1, 2, 3] })).toEqual(hash({ a: [2, 3, 1], b: new Set([3, 2, 1]) }))
  })
})

describe('hashObjectWithoutSorting', () => {
  const hash = hashObjectWithoutSorting
  it('creates a hash', () => {
    expect(hash({ b: 1, a: 2 })).toEqual('mh+rYklpd1DBj/dg6dnG+yd8BQhU2UiUoRMSXjPV1JA=')
    expect(hash(undefined)).toEqual('00000000000000000000000000000000000000000000')
  })
  it('does not sort', () => {
    expect(hash({ b: 1, a: 2 })).not.toEqual(hash({ a: 2, b: 1 }))
    expect(hash({ b: new Set([1, 2, 3]), a: [1, 2, 3] })).not.toEqual(hash({ a: [2, 3, 1], b: new Set([3, 2, 1]) }))
  })
})
