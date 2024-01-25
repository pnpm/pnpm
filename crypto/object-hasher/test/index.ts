import { hashObject, hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'

describe('hashObject', () => {
  it('creates a hash', () => {
    expect(hashObject({ b: 1, a: 2 })).toEqual('c8c943f9321eb7f98834b58391eee848d458c7b35211fc4911cdb1bbd877b74a')
  })
})

describe('hashObjectWithoutSorting', () => {
  it('creates a hash', () => {
    expect(hashObjectWithoutSorting({ b: 1, a: 2 })).toEqual('c0a68b14aa1886a799c2e7c1289b65ccb79a668881d5b6956f0b185a9ac112d7')
  })
})
