import { expect, test } from '@jest/globals'

import { createPackageExtender } from '../lib/createPackageExtender.js'

const packageExtender = createPackageExtender({
  foo: {
    dependencies: {
      a: '1',
    },
    optionalDependencies: {
      b: '2',
    },
    peerDependencies: {
      c: '3',
    },
    peerDependenciesMeta: {
      c: {
        optional: true,
      },
    },
  },
  'bar@1': {
    dependencies: {
      d: '1',
    },
  },
  qar: {
    dependencies: {
      e: '1',
      f: '1',
    },
  },
})

test('createPackageExtender() extends the supported fields', () => {
  expect(
    packageExtender({
      name: 'foo',
      dependencies: {
        bar: '^1.0.0',
      },
    })
  ).toStrictEqual({
    name: 'foo',
    dependencies: {
      bar: '^1.0.0',
      a: '1',
    },
    optionalDependencies: {
      b: '2',
    },
    peerDependencies: {
      c: '3',
    },
    peerDependenciesMeta: {
      c: {
        optional: true,
      },
    },
  })
})

test('createPackageExtender() does not change packages that should not be extended', () => {
  const manifest = { name: 'ignore' }
  expect(packageExtender(manifest)).toStrictEqual(manifest)
})

test('createPackageExtender() matches by version', () => {
  expect(
    packageExtender({
      name: 'bar',
      version: '1.0.0',
    })
  ).toStrictEqual({
    name: 'bar',
    version: '1.0.0',
    dependencies: {
      d: '1',
    },
  })
})

test('createPackageExtender() does not override existing fields', () => {
  expect(
    packageExtender({
      name: 'qar',
      version: '1.0.0',
      dependencies: {
        e: '100',
      },
    })
  ).toStrictEqual({
    name: 'qar',
    version: '1.0.0',
    dependencies: {
      e: '100',
      f: '1',
    },
  })
})

test('createPackageExtender() should works for odd names', () => {
  const extender = createPackageExtender({
    constructor: {
      dependencies: {
        foo: '1',
      },
    },
  })
  expect(
    extender({
      name: 'constructor',
      dependencies: {
      },
    })
  ).toStrictEqual({
    name: 'constructor',
    dependencies: {
      foo: '1',
    },
  })
})

test('createPackageExtender() does not match ranged selectors when version is undefined', () => {
  const extender = createPackageExtender({
    'foo@<2': {
      dependencies: {
        a: '1',
      },
    },
    'foo@*': {
      dependencies: {
        b: '1',
      },
    },
    'foo@^1.0.0': {
      dependencies: {
        c: '1',
      },
    },
    foo: {
      dependencies: {
        bare: '1',
      },
    },
  })
  expect(
    extender({
      name: 'foo',
    } as never)
  ).toStrictEqual({
    name: 'foo',
    dependencies: {
      bare: '1',
    },
  })
})

test('createPackageExtender() matches genuine 0.0.0 with less-than and star selectors', () => {
  const extender = createPackageExtender({
    'foo@<2': {
      dependencies: {
        a: '1',
      },
    },
    'foo@*': {
      dependencies: {
        b: '1',
      },
    },
  })
  expect(
    extender({
      name: 'foo',
      version: '0.0.0',
    } as never)
  ).toStrictEqual({
    name: 'foo',
    version: '0.0.0',
    dependencies: {
      a: '1',
      b: '1',
    },
  })
})
