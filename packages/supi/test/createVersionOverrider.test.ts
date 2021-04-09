import createVersionOverrider from 'supi/lib/install/createVersionsOverrider'

test('createVersionsOverrider() overrides dependencies of specified packages only', () => {
  const overrider = createVersionOverrider({
    'foo@1>bar@^1.2.0': '3.0.0',
  })
  expect(overrider({
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      bar: '^1.2.0',
    },
  })).toStrictEqual({
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      bar: '3.0.0',
    },
  })
  expect(overrider({
    name: 'foo',
    version: '2.0.0',
    dependencies: {
      bar: '^1.2.0',
    },
  })).toStrictEqual({
    name: 'foo',
    version: '2.0.0',
    dependencies: {
      bar: '^1.2.0',
    },
  })
})

test('createVersionsOverrider() overrides all types of dependencies', () => {
  const overrider = createVersionOverrider({
    foo: '3.0.0',
    bar: '3.0.0',
    qar: '3.0.0',
  })
  expect(overrider({
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      foo: '^1.2.0',
    },
    optionalDependencies: {
      bar: '^1.2.0',
    },
    devDependencies: {
      qar: '^1.2.0',
    },
  })).toStrictEqual({
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      foo: '3.0.0',
    },
    optionalDependencies: {
      bar: '3.0.0',
    },
    devDependencies: {
      qar: '3.0.0',
    },
  })
})
