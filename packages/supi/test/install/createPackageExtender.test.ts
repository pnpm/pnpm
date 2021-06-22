import createPackageExtender from 'supi/lib/install/createPackageExtender'

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
