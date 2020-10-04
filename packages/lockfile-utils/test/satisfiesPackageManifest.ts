import { satisfiesPackageManifest } from '@pnpm/lockfile-utils'

const DEFAULT_LOCKFILE_FIELDS = {
  lockfileVersion: 3,
}

const DEFAULT_PKG_FIELDS = {
  name: 'project',
  version: '1.0.0',
}

test('satisfiesPackageManifest()', () => {
  expect(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0' },
  }, '.')).toBe(true)
  expect(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        devDependencies: {},
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0' },
  }, '.')).toBe(true)
  expect(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        devDependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    devDependencies: { foo: '^1.0.0' },
  }, '.')).toBe(true)
  expect(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        optionalDependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    optionalDependencies: { foo: '^1.0.0' },
  }, '.')).toBe(true)
  expect(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    optionalDependencies: { foo: '^1.0.0' },
  }, '.')).toBe(false)
  expect(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.1.0' },
  }, '.')).toBe(false)
  expect(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0', bar: '2.0.0' },
  }, '.')).toBe(false)

  expect(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0', bar: '2.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0', bar: '2.0.0' },
  }, '.')).toBe(false)

  {
    const lockfile = {
      ...DEFAULT_LOCKFILE_FIELDS,
      importers: {
        '.': {
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
    expect(satisfiesPackageManifest(lockfile, pkg, '.')).toBe(true)
  }

  {
    const lockfile = {
      ...DEFAULT_LOCKFILE_FIELDS,
      importers: {
        '.': {
          dependencies: {
            bar: '2.0.0',
            qar: '1.0.0',
          },
          specifiers: {
            bar: '2.0.0',
            qar: '^1.0.0',
          },
        },
      },
    }
    const pkg = {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        bar: '2.0.0',
      },
    }
    expect(satisfiesPackageManifest(lockfile, pkg, '.')).toBe(false)
  }

  {
    const lockfile = {
      ...DEFAULT_LOCKFILE_FIELDS,
      importers: {
        '.': {
          dependencies: {
            bar: '2.0.0',
            qar: '1.0.0',
          },
          specifiers: {
            bar: '2.0.0',
          },
        },
      },
    }
    const pkg = {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        bar: '2.0.0',
      },
    }
    expect(satisfiesPackageManifest(lockfile, pkg, '.')).toBe(false)
  }

  expect(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0', linked: 'link:../linked' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0' },
  }, '.')).toBe(true)

  expect(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      'packages/foo': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0' },
  }, '.')).toBe(false)

  expect(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: {
          foo: '1.0.0',
        },
        specifiers: {
          foo: '1.0.0',
        },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: {
      foo: '1.0.0',
    },
    devDependencies: {
      foo: '1.0.0',
    },
  }, '.')).toBe(true)
})
