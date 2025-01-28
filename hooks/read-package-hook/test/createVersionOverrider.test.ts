import path from 'path'
import { createVersionsOverrider } from '../src/createVersionsOverrider'

test('createVersionsOverrider() matches sub-ranges', () => {
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'foo',
        pref: '2',
      },
      newPref: '2.12.0',
    },
    {
      targetPkg: {
        name: 'qar',
        pref: '>2',
      },
      newPref: '1.0.0',
    },
  ], process.cwd())
  expect(
    overrider({
      dependencies: { foo: '^2.10.0' },
      optionalDependencies: { qar: '^4.0.0' },
    })
  ).toStrictEqual({
    dependencies: { foo: '2.12.0' },
    optionalDependencies: { qar: '1.0.0' },
  })
})

test('createVersionsOverrider() does not fail on non-range selectors', () => {
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'foo',
        pref: '2',
      },
      newPref: '2.12.0',
    },
    {
      targetPkg: {
        name: 'bar',
        pref: 'github:org/bar',
      },
      newPref: '2.12.0',
    },
  ], process.cwd())
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
  const overrider = createVersionsOverrider([
    {
      parentPkg: {
        name: 'foo',
        pref: '1',
      },
      targetPkg: {
        name: 'bar',
        pref: '^1.2.0',
      },
      newPref: '3.0.0',
    },
    {
      parentPkg: {
        name: 'qar',
        pref: '1',
      },
      targetPkg: {
        name: 'bar',
        pref: '>4',
      },
      newPref: '3.0.0',
    },
  ], process.cwd())
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
  expect(overrider({
    name: 'qar',
    version: '1.0.0',
    dependencies: {
      bar: '^10.0.0',
    },
  })).toStrictEqual({
    name: 'qar',
    version: '1.0.0',
    dependencies: {
      bar: '3.0.0',
    },
  })
  expect(overrider({
    name: 'qar',
    version: '1.0.0',
    dependencies: {
      bar: '^4.0.0',
    },
  })).toStrictEqual({
    name: 'qar',
    version: '1.0.0',
    dependencies: {
      bar: '^4.0.0',
    },
  })
})

test('createVersionsOverrider() overrides all types of dependencies', () => {
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'foo',
      },
      newPref: '3.0.0',
    },
    {
      targetPkg: {
        name: 'bar',
      },
      newPref: '3.0.0',
    },
    {
      targetPkg: {
        name: 'qar',
      },
      newPref: '3.0.0',
    },
  ], process.cwd())
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
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'qar',
      },
      newPref: 'link:../qar',
    },
  ], process.cwd())
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

test('createVersionsOverrider() overrides dependencies with absolute links', () => {
  const qarAbsolutePath = path.resolve(process.cwd(), './qar')
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'qar',
      },
      newPref: `link:${qarAbsolutePath}`,
    },
  ], process.cwd())

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
      qar: `link:${qarAbsolutePath}`,
    },
  })
})

test('createVersionsOverrider() overrides dependency of pkg matched by name and version', () => {
  const overrider = createVersionsOverrider([
    {
      parentPkg: {
        name: 'yargs',
        pref: '^7.1.0',
      },
      targetPkg: {
        name: 'yargs-parser',
      },
      newPref: '^20.0.0',
    },
  ], process.cwd())
  expect(
    overrider({
      name: 'yargs',
      version: '7.1.0',
      dependencies: {
        'yargs-parser': '19',
      },
    })
  ).toStrictEqual({
    name: 'yargs',
    version: '7.1.0',
    dependencies: {
      'yargs-parser': '^20.0.0',
    },
  })
})

test('createVersionsOverrider() does not override dependency of pkg matched by name and version', () => {
  const overrider = createVersionsOverrider([
    {
      parentPkg: {
        name: 'yargs',
        pref: '^8.1.0',
      },
      targetPkg: {
        name: 'yargs-parser',
      },
      newPref: '^20.0.0',
    },
  ], process.cwd())
  expect(
    overrider({
      name: 'yargs',
      version: '7.1.0',
      dependencies: {
        'yargs-parser': '19',
      },
    })
  ).toStrictEqual({
    name: 'yargs',
    version: '7.1.0',
    dependencies: {
      'yargs-parser': '19',
    },
  })
})

test('createVersionsOverrider() should work for scoped parent and unscoped child', () => {
  const overrider = createVersionsOverrider([
    {
      parentPkg: {
        name: '@scoped/package',
      },
      targetPkg: {
        name: 'unscoped-package',
      },
      newPref: 'workspace:*',
    },
  ], process.cwd())
  expect(
    overrider({
      name: '@scoped/package',
      version: '1.0.0',
      dependencies: {
        'unscoped-package': '1.0.0',
      },
    })
  ).toStrictEqual({
    name: '@scoped/package',
    version: '1.0.0',
    dependencies: {
      'unscoped-package': 'workspace:*',
    },
  })
})

test('createVersionsOverrider() should work for unscoped parent and scoped child', () => {
  const overrider = createVersionsOverrider([
    {
      parentPkg: {
        name: 'unscoped-package',
      },
      targetPkg: {
        name: '@scoped/package',
      },
      newPref: 'workspace:*',
    },
  ], process.cwd())
  expect(
    overrider({
      name: 'unscoped-package',
      version: '1.0.0',
      dependencies: {
        '@scoped/package': '1.0.0',
      },
    })
  ).toStrictEqual({
    name: 'unscoped-package',
    version: '1.0.0',
    dependencies: {
      '@scoped/package': 'workspace:*',
    },
  })
})

test('createVersionsOverrider() should work for scoped parent and scoped child', () => {
  const overrider = createVersionsOverrider([
    {
      parentPkg: {
        name: '@scoped/package',
      },
      targetPkg: {
        name: '@scoped/package2',
      },
      newPref: 'workspace:*',
    },
  ], process.cwd())
  expect(
    overrider({
      name: '@scoped/package',
      version: '1.0.0',
      dependencies: {
        '@scoped/package2': '1.0.0',
      },
    })
  ).toStrictEqual({
    name: '@scoped/package',
    version: '1.0.0',
    dependencies: {
      '@scoped/package2': 'workspace:*',
    },
  })
})

test('createVersionsOverrider() overrides dependencies with file with relative path for root package', () => {
  const rootDir = process.cwd()
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'qar',
      },
      newPref: 'file:../qar',
    },
  ], rootDir)
  expect(overrider({
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      qar: '3.0.0',
    },
  }, rootDir)).toStrictEqual({
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      qar: 'file:../qar',
    },
  })
})

test('createVersionsOverrider() overrides dependencies with file with relative path for workspace package', () => {
  const rootDir = process.cwd()
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'qar',
      },
      newPref: 'file:../qar',
    },
  ], rootDir)
  expect(overrider({
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      qar: '3.0.0',
    },
  }, path.join(rootDir, 'packages', 'pkg'))).toStrictEqual({
    name: 'foo',
    version: '1.2.0',
    dependencies: {
      qar: 'file:../../../qar',
    },
  })
})

test('createVersionsOverrider() overrides dependencies with file specified with absolute path', () => {
  const absolutePath = path.join(__dirname, 'qar')
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'qar',
      },
      newPref: `file:${absolutePath}`,
    },
  ], process.cwd())
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
      qar: `file:${absolutePath}`,
    },
  })
})

test('createVersionOverride() should use the most specific rule when both override rules match the same target', () => {
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'foo',
      },
      newPref: '3.0.0',
    },
    {
      targetPkg: {
        name: 'foo',
        pref: '3',
      },
      newPref: '4.0.0',
    },
    {
      targetPkg: {
        name: 'foo',
        pref: '2',
      },
      newPref: '2.12.0',
    },
    {
      parentPkg: {
        name: 'bar',
      },
      targetPkg: {
        name: 'foo',
        pref: '2',
      },
      newPref: 'github:org/foo',
    },
    {
      parentPkg: {
        name: 'bar',
      },
      targetPkg: {
        name: 'foo',
        pref: '3',
      },
      newPref: '5.0.0',
    },
  ], process.cwd())
  expect(
    overrider({
      dependencies: {
        foo: '^3.0.0',
      },
    })
  ).toStrictEqual({
    dependencies: {
      foo: '4.0.0',
    },
  })
  expect(
    overrider({
      dependencies: {
        foo: '^4.0.0',
      },
    })
  ).toStrictEqual({
    dependencies: {
      foo: '3.0.0',
    },
  })
  expect(
    overrider({
      dependencies: {
        foo: '^2.0.0',
      },
    })
  ).toStrictEqual({
    dependencies: {
      foo: '2.12.0',
    },
  })
  expect(
    overrider({
      name: 'bar',
      version: '1.0.0',
      dependencies: {
        foo: '^2.0.0',
      },
    })
  ).toStrictEqual({
    name: 'bar',
    version: '1.0.0',
    dependencies: {
      foo: 'github:org/foo',
    },
  })
  expect(
    overrider({
      name: 'bar',
      version: '1.0.0',
      dependencies: {
        foo: '^3.0.0',
      },
    })
  ).toStrictEqual({
    name: 'bar',
    version: '1.0.0',
    dependencies: {
      foo: '5.0.0',
    },
  })
})

test('createVersionsOverrider() matches intersections', () => {
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'foo',
        pref: '<1.2.4',
      },
      newPref: '>=1.2.4',
    },
  ], process.cwd())
  expect(
    overrider({
      dependencies: { foo: '^1.2.3' },
    })
  ).toStrictEqual({
    dependencies: { foo: '>=1.2.4' },
  })
})

test('createVersionsOverrider() overrides peerDependencies of another dependency', () => {
  const overrider = createVersionsOverrider([
    {
      parentPkg: {
        name: 'react-dom',
      },
      targetPkg: {
        name: 'react',
      },
      newPref: '18.1.0',
    },
  ], process.cwd())
  expect(
    overrider({
      name: 'react-dom',
      version: '18.2.0',
      peerDependencies: {
        react: '18.2.0',
      },
    })
  ).toStrictEqual({
    name: 'react-dom',
    version: '18.2.0',
    dependencies: {},
    peerDependencies: {
      react: '18.1.0',
    },
  })
})

test('createVersionsOverrider() removes dependencies', () => {
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'foo',
      },
      newPref: '-',
    },
    {
      parentPkg: {
        name: 'bar',
      },
      targetPkg: {
        name: 'baz',
      },
      newPref: '-',
    },
    {
      targetPkg: {
        name: 'qux',
        pref: '2',
      },
      newPref: '-',
    },
  ], process.cwd())
  expect(
    overrider({
      dependencies: {
        foo: '0.1.2',
        bar: '1.2.3',
        baz: '1.0.0',
        qux: '2.1.0',
      },
    })
  ).toStrictEqual({
    dependencies: {
      bar: '1.2.3',
      baz: '1.0.0',
    },
  })
  expect(
    overrider({
      name: 'bar',
      dependencies: {
        foo: '0.1.2',
        bar: '1.2.3',
        baz: '1.0.0',
        qux: '2.1.0',
      },
    })
  ).toStrictEqual({
    name: expect.anything(),
    dependencies: {
      bar: '1.2.3',
    },
  })
  expect(
    overrider({
      dependencies: {
        foo: '0.1.2',
        bar: '1.2.3',
        baz: '1.0.0',
        qux: '3.2.1',
      },
    })
  ).toStrictEqual({
    dependencies: {
      bar: '1.2.3',
      baz: '1.0.0',
      qux: '3.2.1',
    },
  })
})

test('createVersionsOverrider() moves invalid versions from peerDependencies to dependencies', () => {
  const overrider = createVersionsOverrider([
    {
      targetPkg: {
        name: 'foo',
      },
      newPref: 'link:foo',
    },
    {
      targetPkg: {
        name: 'bar',
      },
      newPref: 'file:bar',
    },
    {
      targetPkg: {
        name: 'baz',
      },
      newPref: '7.7.7',
    },
  ], process.cwd())
  expect(
    overrider({
      peerDependencies: {
        foo: '^1.0.0 || ^2.0.0',
        bar: '^1.0.0 || ^2.0.0',
        baz: '^1.0.0 || ^2.0.0',
        qux: '^1.0.0 || ^2.0.0',
      },
    })
  ).toStrictEqual({
    dependencies: {
      foo: expect.stringMatching(/^link:.*foo[/\\]?$/),
      bar: expect.stringMatching(/^file:.*bar[/\\]?$/),
    },
    peerDependencies: {
      bar: '^1.0.0 || ^2.0.0',
      baz: '7.7.7',
      foo: '^1.0.0 || ^2.0.0',
      qux: '^1.0.0 || ^2.0.0',
    },
  })
  expect(
    overrider({
      dependencies: {
        foo: '^1.0.0',
        bar: '^2.0.0',
        baz: '^1.2.3',
        qux: '^2.1.0',
      },
      peerDependencies: {
        foo: '^1.0.0 || ^2.0.0',
        bar: '^1.0.0 || ^2.0.0',
        baz: '^1.0.0 || ^2.0.0',
        qux: '^1.0.0 || ^2.0.0',
      },
    })
  ).toStrictEqual({
    dependencies: {
      foo: expect.stringMatching(/^link:.*foo[/\\]?$/),
      bar: expect.stringMatching(/^file:.*bar[/\\]?$/),
      baz: '7.7.7',
      qux: '^2.1.0',
    },
    peerDependencies: {
      bar: '^1.0.0 || ^2.0.0',
      baz: '7.7.7',
      foo: '^1.0.0 || ^2.0.0',
      qux: '^1.0.0 || ^2.0.0',
    },
  })
})
