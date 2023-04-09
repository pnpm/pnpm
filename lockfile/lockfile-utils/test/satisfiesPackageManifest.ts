import { satisfiesPackageManifest } from '@pnpm/lockfile-utils'

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
  )).toBe(true)
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
  )).toBe(true)
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
  )).toBe(true)
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
  )).toBe(true)
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
  )).toBe(false)
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
  )).toBe(false)
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
  )).toBe(false)

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
  )).toBe(false)

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
    expect(satisfiesPackageManifest({}, importer, pkg)).toBe(true)
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
    expect(satisfiesPackageManifest({}, importer, pkg)).toBe(false)
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
    expect(satisfiesPackageManifest({}, importer, pkg)).toBe(false)
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
  )).toBe(true)

  expect(satisfiesPackageManifest(
    {},
    undefined,
    {
      ...DEFAULT_PKG_FIELDS,
      dependencies: { foo: '^1.0.0' },
    }
  )).toBe(false)

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
  )).toBe(true)

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
  )).toBe(true)

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
  )).toBe(true)

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
  )).toBe(true)

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
  )).toBe(true)

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
  )).toBe(false)

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
  )).toBe(true)
})
