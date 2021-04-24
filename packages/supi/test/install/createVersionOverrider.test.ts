import path from 'path'
import createVersionsOverrider from 'supi/lib/install/createVersionsOverrider'

test('createVersionsOverrider() matches subranges', () => {
  const overrider = createVersionsOverrider({
    'foo@2': '2.12.0',
  }, process.cwd())
  expect(
    overrider({
      dependencies: { foo: '^2.10.0' },
    })
  ).toStrictEqual({ dependencies: { foo: '2.12.0' } })
})

test('createVersionsOverrider() does not fail on non-range selectors', () => {
  const overrider = createVersionsOverrider({
    'foo@2': '2.12.0',
    'bar@github:org/bar': '2.12.0',
  }, process.cwd())
  expect(
    overrider({
      dependencies: {
        foo: 'github:org/foo',
        bar: 'github:org/bar',
      },
    })
  ).toStrictEqual({
    dependencies: {
      foo: 'github:org/foo',
      bar: '2.12.0',
    },
  })
})

test('createVersionsOverrider() overrides dependencies of specified packages only', () => {
  const overrider = createVersionsOverrider({
    'foo@1>bar@^1.2.0': '3.0.0',
  }, process.cwd())
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
  const overrider = createVersionsOverrider({
    foo: '3.0.0',
    bar: '3.0.0',
    qar: '3.0.0',
  }, process.cwd())
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

test('createVersionsOverrider() overrides dependencies with links', () => {
  const overrider = createVersionsOverrider({
    qar: 'link:../qar',
  }, process.cwd())
  expect(overrider({
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      qar: '3.0.0',
    },
  }, path.resolve('pkg'))).toStrictEqual({
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      qar: 'link:../../qar',
    },
  })
})
