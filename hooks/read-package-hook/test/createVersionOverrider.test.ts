import path from 'path'
import { createVersionsOverrider } from '../lib/createVersionsOverrider'
import normalizePath from 'normalize-path'

test('createVersionsOverrider() matches sub-ranges', () => {
  const overrider = createVersionsOverrider({
    'foo@2': '2.12.0',
    'qar@>2': '1.0.0',
  }, process.cwd())
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
    'qar@1>bar@>4': '3.0.0',
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

test('createVersionsOverrider() overrides dependencies with absolute links', () => {
  const qarAbsolutePath = path.resolve(process.cwd(), './qar')
  const overrider = createVersionsOverrider({
    qar: `link:${qarAbsolutePath}`,
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
      qar: `link:${normalizePath(path.relative(path.resolve('pkg'), qarAbsolutePath))}`,
    },
  })
})

test('createVersionsOverrider() overrides dependency of pkg matched by name and version', () => {
  const overrider = createVersionsOverrider({
    'yargs@^7.1.0>yargs-parser': '^20.0.0',
  }, process.cwd())
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
  const overrider = createVersionsOverrider({
    'yargs@^8.1.0>yargs-parser': '^20.0.0',
  }, process.cwd())
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
  const overrider = createVersionsOverrider({
    '@scoped/package>unscoped-package': 'workspace:*',
  }, process.cwd())
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
  const overrider = createVersionsOverrider({
    'unscoped-package>@scoped/package': 'workspace:*',
  }, process.cwd())
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
  const overrider = createVersionsOverrider({
    '@scoped/package>@scoped/package2': 'workspace:*',
  }, process.cwd())
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

test('createVersionsOverrider() overrides dependencies with file', () => {
  const overrider = createVersionsOverrider({
    qar: 'file:../qar',
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
      qar: `file:${path.resolve('../qar')}`,
    },
  })
})

test('createVersionsOverrider() overrides dependencies with file specified with absolute path', () => {
  const absolutePath = path.join(__dirname, 'qar')
  const overrider = createVersionsOverrider({
    qar: `file:${absolutePath}`,
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
      qar: `file:${absolutePath}`,
    },
  })
})

test('createVersionOverride() should use the most specific rule when both override rules match the same target', () => {
  const overrider = createVersionsOverrider({
    foo: '3.0.0',
    'foo@3': '4.0.0',
    'foo@2': '2.12.0',
    'bar>foo@2': 'github:org/foo',
    'bar>foo@3': '5.0.0',
  }, process.cwd())
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

test('createVersionOverrider() throws error when supplied an invalid selector', () => {
  expect(() => createVersionsOverrider({
    'foo > bar': '2',
  }, process.cwd())).toThrowError('Cannot parse the "foo > bar" selector in pnpm.overrides')
})

test('createVersionsOverrider() matches intersections', () => {
  const overrider = createVersionsOverrider({
    'foo@<1.2.4': '>=1.2.4',
  }, process.cwd())
  expect(
    overrider({
      dependencies: { foo: '^1.2.3' },
    })
  ).toStrictEqual({
    dependencies: { foo: '>=1.2.4' },
  })
})

test('createVersionsOverrider() overrides peerDependencies of another dependency', () => {
  const overrider = createVersionsOverrider({
    'react-dom>react': '18.1.0',
  }, process.cwd())
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
    peerDependencies: {
      react: '18.1.0',
    },
  })
})
