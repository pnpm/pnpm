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
