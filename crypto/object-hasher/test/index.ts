import { hashObject, hashObjectWithoutSorting, createPackageExtensionsChecksum } from '@pnpm/crypto.object-hasher'

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

describe('createPackageExtensionsChecksum', () => {
  const hash = createPackageExtensionsChecksum
  it('creates a hash', () => {
    expect(hash({
      foo: {
        dependencies: {
          abc: '0.1.2',
          def: '3.4.5',
        },
      },
      bar: {
        dependencies: {
          abc: '0.1.2',
        },
        peerDependencies: {
          def: '3.4.5',
        },
      },
    })).toStrictEqual('sha256-rQFUgJKDgN5oCbSKGAfYurLFKkdX/NaD9VjnBPBz4CI=')
    expect(hash({})).toStrictEqual(undefined)
    expect(hash(undefined)).toStrictEqual(undefined)
  })
  it('sorts', () => {
    expect(hash({
      foo: {
        dependencies: {
          abc: '0.1.2',
          def: '3.4.5',
        },
      },
      bar: {
        dependencies: {
          abc: '0.1.2',
        },
        peerDependencies: {
          def: '3.4.5',
        },
      },
    })).toStrictEqual(hash({
      bar: {
        peerDependencies: {
          def: '3.4.5',
        },
        dependencies: {
          abc: '0.1.2',
        },
      },
      foo: {
        dependencies: {
          def: '3.4.5',
          abc: '0.1.2',
        },
      },
    }))
  })
})
