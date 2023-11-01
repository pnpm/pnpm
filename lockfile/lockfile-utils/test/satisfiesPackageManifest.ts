import { satisfiesPackageManifest } from '@pnpm/lockfile-utils'

const DEFAULT_PKG_FIELDS = {
  name: 'project',
  version: '1.0.0',
}

test('satisfiesPackageManifest()', () => {
  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.0.0' },
    }
  )).toStrictEqual({ satisfies: true })
  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
      devDependencies: {},
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.0.0' },
    }
  )).toStrictEqual({ satisfies: true })
  expect(satisfiesPackageManifest(
    {},
    {
      devDependencies: {
        foo: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      devDependencies: { foo: '^1.0.0' },
    }
  )).toStrictEqual({ satisfies: true })
  expect(satisfiesPackageManifest(
    {},
    {
      optionalDependencies: {
        foo: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      optionalDependencies: { foo: '^1.0.0' },
    }
  )).toStrictEqual({ satisfies: true })
  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      optionalDependencies: { foo: '^1.0.0' },
    }
  )).toStrictEqual({
    satisfies: false,
    detailedReason: '"optionalDependencies" in the lockfile ({}) doesn\'t match the same field in package.json ({"foo":"^1.0.0"})',
  })
  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.1.0' },
    }
  )).toStrictEqual({
    satisfies: false,
    detailedReason: 'specifier in the lockfile for "foo" in "dependencies" (^1.0.0) don\'t match the spec in package.json (^1.1.0)',
  })
  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.0.0', bar: '2.0.0' },
    }
  )).toStrictEqual({
    satisfies: false,
    detailedReason: '"dependencies" in the lockfile ({"foo":"^1.0.0"}) doesn\'t match the same field in package.json ({"foo":"^1.0.0","bar":"2.0.0"})',
  })

  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
      devDependencies: {
        bar: {
          version: '2.0.0',
          specifier: '^2.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.0.0', bar: '2.0.0' },
    }
  )).toStrictEqual({
    satisfies: false,
    detailedReason: '"dependencies" in the lockfile ({"foo":"^1.0.0"}) doesn\'t match the same field in package.json ({"foo":"^1.0.0","bar":"2.0.0"})',
  })

  {
    const importer = {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
      optionalDependencies: {
        bar: {
          version: '2.0.0',
          specifier: '2.0.0',
        },
      },
    }
    const pkg = {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        bar: '2.0.0',
        foo: '^1.0.0',
      },
      optionalDependencies: {
        bar: '2.0.0',
      },
    }
    expect(satisfiesPackageManifest({}, importer, pkg)).toStrictEqual({ satisfies: true })
  }

  {
    const importer = {
      dependencies: {
        bar: {
          version: '2.0.0',
          specifier: '2.0.0',
        },
        qar: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
    }
    const pkg = {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        bar: '2.0.0',
      },
    }
    expect(satisfiesPackageManifest({}, importer, pkg)).toStrictEqual({
      satisfies: false,
      detailedReason: '"dependencies" in the lockfile ({"bar":"2.0.0","qar":"^1.0.0"}) doesn\'t match the same field in package.json ({"bar":"2.0.0"})',
    })
  }

  {
    const importer = {
      dependencies: {
        bar: {
          version: '2.0.0',
          specifier: '2.0.0',
        },
        qar: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
    }
    const pkg = {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        bar: '2.0.0',
      },
    }
    expect(satisfiesPackageManifest({}, importer, pkg)).toStrictEqual({
      satisfies: false,
      detailedReason: '"dependencies" in the lockfile ({"bar":"2.0.0","qar":"^1.0.0"}) doesn\'t match the same field in package.json ({"bar":"2.0.0"})',
    })
  }

  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
        linked: {
          version: 'link:../linked',
          specifier: 'link:../linked',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.0.0' },
    }
  )).toStrictEqual({ satisfies: true })

  expect(satisfiesPackageManifest(
    {},
    undefined,
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.0.0' },
    }
  )).toStrictEqual({ satisfies: false, detailedReason: 'no importer' })

  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '1.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        foo: '1.0.0',
      },
      devDependencies: {
        foo: '1.0.0',
      },
    }
  )).toStrictEqual({ satisfies: true })

  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '1.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        foo: '1.0.0',
      },
      devDependencies: {
        foo: '1.0.0',
      },
      dependenciesMeta: {},
    }
  )).toStrictEqual({ satisfies: true })

  expect(satisfiesPackageManifest(
    { autoInstallPeers: true },
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '1.0.0',
        },
        bar: {
          version: '1.0.0',
          specifier: '^1.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        foo: '1.0.0',
      },
      peerDependencies: {
        bar: '^1.0.0',
      },
    }
  )).toStrictEqual({ satisfies: true })

  expect(satisfiesPackageManifest(
    { autoInstallPeers: true },
    {
      dependencies: {
        qar: {
          version: '1.0.0',
          specifier: '1.0.0',
        },
      },
      optionalDependencies: {
        bar: {
          version: '1.0.0',
          specifier: '1.0.0',
        },
      },
      devDependencies: {
        foo: {
          version: '1.0.0',
          specifier: '1.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        qar: '1.0.0',
      },
      optionalDependencies: {
        bar: '1.0.0',
      },
      devDependencies: {
        foo: '1.0.0',
      },
      peerDependencies: {
        foo: '^1.0.0',
        bar: '^1.0.0',
        qar: '^1.0.0',
      },
    }
  )).toStrictEqual({ satisfies: true })

  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '1.0.0',
        },
      },
      publishDirectory: 'dist',
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        foo: '1.0.0',
      },
      publishConfig: {
        directory: 'dist',
      },
    }
  )).toStrictEqual({ satisfies: true })

  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '1.0.0',
        },
      },
      publishDirectory: 'dist',
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        foo: '1.0.0',
      },
      publishConfig: {
        directory: 'lib',
      },
    }
  )).toStrictEqual({
    satisfies: false,
    detailedReason: '"publishDirectory" in the lockfile (dist) doesn\'t match "publishConfig.directory" in package.json (lib)',
  })

  expect(satisfiesPackageManifest(
    {
      excludeLinksFromLockfile: true,
    },
    {
      dependencies: {
        foo: {
          version: '1.0.0',
          specifier: '1.0.0',
        },
      },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        foo: '1.0.0',
        bar: 'link:../bar',
      },
    }
  )).toStrictEqual({ satisfies: true })
})
