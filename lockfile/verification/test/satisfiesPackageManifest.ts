import { satisfiesPackageManifest } from '@pnpm/lockfile.verification'

const DEFAULT_PKG_FIELDS = {
  name: 'project',
  version: '1.0.0',
}

test('satisfiesPackageManifest()', () => {
  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: { foo: '1.0.0' },
      specifiers: { foo: '^1.0.0' },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.0.0' },
    }
  )).toStrictEqual({ satisfies: true })
  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: { foo: '1.0.0' },
      devDependencies: {},
      specifiers: { foo: '^1.0.0' },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.0.0' },
    }
  )).toStrictEqual({ satisfies: true })
  expect(satisfiesPackageManifest(
    {},
    {
      devDependencies: { foo: '1.0.0' },
      specifiers: { foo: '^1.0.0' },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      devDependencies: { foo: '^1.0.0' },
    }
  )).toStrictEqual({ satisfies: true })
  expect(satisfiesPackageManifest(
    {},
    {
      optionalDependencies: { foo: '1.0.0' },
      specifiers: { foo: '^1.0.0' },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      optionalDependencies: { foo: '^1.0.0' },
    }
  )).toStrictEqual({ satisfies: true })
  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: { foo: '1.0.0' },
      specifiers: { foo: '^1.0.0' },
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
      dependencies: { foo: '1.0.0' },
      specifiers: { foo: '^1.0.0' },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.1.0' },
    }
  )).toStrictEqual({
    satisfies: false,
    detailedReason: `specifiers in the lockfile don't match specifiers in package.json:
* 1 dependencies are mismatched:
  - foo (lockfile: ^1.0.0, manifest: ^1.1.0)
`,
  })
  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: { foo: '1.0.0' },
      specifiers: { foo: '^1.0.0' },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.0.0', bar: '2.0.0' },
    }
  )).toStrictEqual({
    satisfies: false,
    detailedReason: `specifiers in the lockfile don't match specifiers in package.json:
* 1 dependencies were added: bar@2.0.0
`,
  })

  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: { foo: '1.0.0' },
      specifiers: { foo: '^1.0.0', bar: '2.0.0' },
    },
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.0.0', bar: '2.0.0' },
    }
  )).toStrictEqual({
    satisfies: false,
    detailedReason: '"dependencies" in the lockfile ({"foo":"1.0.0"}) doesn\'t match the same field in package.json ({"foo":"^1.0.0","bar":"2.0.0"})',
  })

  {
    const importer = {
      dependencies: {
        foo: '1.0.0',
      },
      optionalDependencies: {
        bar: '2.0.0',
      },
      specifiers: {
        bar: '2.0.0',
        foo: '^1.0.0',
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
        bar: '2.0.0',
        qar: '1.0.0',
      },
      specifiers: {
        bar: '2.0.0',
        qar: '^1.0.0',
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
      detailedReason: `specifiers in the lockfile don't match specifiers in package.json:
* 1 dependencies were removed: qar@^1.0.0
`,
    })
  }

  {
    const importer = {
      dependencies: {
        bar: '2.0.0',
        qar: '1.0.0',
      },
      specifiers: {
        bar: '2.0.0',
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
      detailedReason: '"dependencies" in the lockfile ({"bar":"2.0.0","qar":"1.0.0"}) doesn\'t match the same field in package.json ({"bar":"2.0.0"})',
    })
  }

  expect(satisfiesPackageManifest(
    {},
    {
      dependencies: { foo: '1.0.0', linked: 'link:../linked' },
      specifiers: { foo: '^1.0.0' },
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
        foo: '1.0.0',
      },
      specifiers: {
        foo: '1.0.0',
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
        foo: '1.0.0',
      },
      specifiers: {
        foo: '1.0.0',
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
        foo: '1.0.0',
        bar: '1.0.0',
      },
      specifiers: {
        foo: '1.0.0',
        bar: '^1.0.0',
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
        qar: '1.0.0',
      },
      optionalDependencies: {
        bar: '1.0.0',
      },
      devDependencies: {
        foo: '1.0.0',
      },
      specifiers: {
        foo: '1.0.0',
        bar: '1.0.0',
        qar: '1.0.0',
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
        foo: '1.0.0',
      },
      specifiers: {
        foo: '1.0.0',
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
        foo: '1.0.0',
      },
      specifiers: {
        foo: '1.0.0',
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
        foo: '1.0.0',
      },
      specifiers: {
        foo: '1.0.0',
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

  expect(satisfiesPackageManifest({}, {
    dependencies: {
      '@apollo/client': '3.13.8(@types/react@18.3.23)(graphql@15.8.0)(react-dom@17.0.2(react@17.0.2))(react@17.0.2)(subscriptions-transport-ws@0.11.0(graphql@15.8.0))',
    },
    specifiers: {
      '@apollo/client': '3.3.7',
    },
  }, {
    dependencies: {
      '@apollo/client': '3.3.7',
    },
  })).toStrictEqual({
    satisfies: false,
    detailedReason: 'The importer resolution is broken at dependency "@apollo/client": version "3.13.8" doesn\'t satisfy range "3.3.7"',
  })
})
