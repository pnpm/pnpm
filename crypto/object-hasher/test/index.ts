import { hashObject, hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'

describe('hashObject', () => {
  const hash = hashObject
  it('creates a hash', () => {
    expect(hash({ b: 1, a: 2 })).toEqual('e3d3f89836fac144779e57d0e831efd06336036b')
    expect(hash(undefined)).toEqual('0000000000000000000000000000000000000000')
  })
  it('sorts', () => {
    expect(hash({ b: 1, a: 2 })).toEqual(hash({ a: 2, b: 1 }))
    expect(hash({ b: new Set([1, 2, 3]), a: [1, 2, 3] })).toEqual(hash({ a: [2, 3, 1], b: new Set([3, 2, 1]) }))
  })
})

describe('hashObjectWithoutSorting', () => {
  const hash = hashObjectWithoutSorting
  it('creates a hash', () => {
    expect(hash({ b: 1, a: 2 })).toEqual('dd34c1644a1d52da41808e5c1e6849829ef77999')
    expect(hash(undefined)).toEqual('0000000000000000000000000000000000000000')
  })
  it('does not sort', () => {
    expect(hash({ b: 1, a: 2 })).not.toEqual(hash({ a: 2, b: 1 }))
    expect(hash({ b: new Set([1, 2, 3]), a: [1, 2, 3] })).not.toEqual(hash({ a: [2, 3, 1], b: new Set([3, 2, 1]) }))
  })
})
