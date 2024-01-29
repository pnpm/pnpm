import { hashObject, hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'

describe('hashObject', () => {
  const hash = hashObject
  it('creates a hash', () => {
    expect(hash({ b: 1, a: 2 })).toEqual('c8c943f9321eb7f98834b58391eee848d458c7b35211fc4911cdb1bbd877b74a')
    expect(hash(undefined)).toEqual('e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855')
  })
  it('sorts', () => {
    expect(hash({ b: 1, a: 2 })).toEqual(hash({ a: 2, b: 1 }))
    expect(hash({ b: new Set([1, 2, 3]), a: [1, 2, 3] })).toEqual(hash({ a: [2, 3, 1], b: new Set([3, 2, 1]) }))
  })
})

describe('hashObjectWithoutSorting', () => {
  const hash = hashObjectWithoutSorting
  it('creates a hash', () => {
    expect(hash({ b: 1, a: 2 })).toEqual('c0a68b14aa1886a799c2e7c1289b65ccb79a668881d5b6956f0b185a9ac112d7')
    expect(hash(undefined)).toEqual('e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855')
  })
  it('does not sort', () => {
    expect(hash({ b: 1, a: 2 })).not.toEqual(hash({ a: 2, b: 1 }))
    expect(hash({ b: new Set([1, 2, 3]), a: [1, 2, 3] })).not.toEqual(hash({ a: [2, 3, 1], b: new Set([3, 2, 1]) }))
  })
})
