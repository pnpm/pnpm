import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'

import { mergeLockfileChanges } from '../src/index.js'

const simpleLockfile = {
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
  lockfileVersion: '5.2',
  packages: {
    '/foo@1.0.0': {
      resolution: {
        integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
      },
    },
  },
}

test('picks the newer version when dependencies differ inside importer', () => {
  const mergedLockfile = mergeLockfileChanges(
    {
      ...simpleLockfile,
      importers: {
        ['.' as ProjectId]: {
          ...simpleLockfile.importers['.'],
          dependencies: {
            foo: '1.2.0',
            bar: '3.0.0(qar@1.0.0)',
            zoo: '4.0.0(qar@1.0.0)',
          },
        },
      },
    },
    {
      ...simpleLockfile,
      importers: {
        ['.' as ProjectId]: {
          ...simpleLockfile.importers['.'],
          dependencies: {
            foo: '1.1.0',
            bar: '4.0.0(qar@1.0.0)',
            zoo: '3.0.0(qar@1.0.0)',
          },
        },
      },
    }
  )
  expect(mergedLockfile.importers['.' as ProjectId].dependencies?.foo).toBe('1.2.0')
  expect(mergedLockfile.importers['.' as ProjectId].dependencies?.bar).toBe('4.0.0(qar@1.0.0)')
  expect(mergedLockfile.importers['.' as ProjectId].dependencies?.zoo).toBe('4.0.0(qar@1.0.0)')
})

test('picks the newer version when dependencies differ inside package', () => {
  const base: LockfileObject = {
    importers: {
      ['.' as ProjectId]: {
        dependencies: {
          a: '1.0.0',
        },
        specifiers: {},
      },
    },
    lockfileVersion: '5.2',
    packages: {
      ['/a@1.0.0' as DepPath]: {
        dependencies: {
          foo: '1.0.0',
        },
        resolution: {
          integrity: '',
        },
      },
      ['/foo@1.0.0' as DepPath]: {
        resolution: {
          integrity: '',
        },
      },
    },
  }
  const mergedLockfile = mergeLockfileChanges(
    {
      ...base,
      packages: {
        ...base.packages,
        ['/a@1.0.0' as DepPath]: {
          dependencies: {
            linked: 'link:../1',
            foo: '1.2.0',
            bar: '3.0.0(qar@1.0.0)',
            zoo: '4.0.0(qar@1.0.0)',
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        ['/bar@3.0.0(qar@1.0.0)' as DepPath]: {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        ['/zoo@4.0.0(qar@1.0.0)' as DepPath]: {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        ['/foo@1.2.0' as DepPath]: {
          resolution: {
            integrity: '',
          },
        },
        ['/qar@1.0.0' as DepPath]: {
          resolution: {
            integrity: '',
          },
        },
      },
    },
    {
      ...base,
      packages: {
        ...base.packages,
        ['/a@1.0.0' as DepPath]: {
          dependencies: {
            linked: 'link:../1',
            foo: '1.1.0',
            bar: '4.0.0(qar@1.0.0)',
            zoo: '3.0.0(qar@1.0.0)',
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        ['/bar@4.0.0(qar@1.0.0)' as DepPath]: {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        ['/zoo@3.0.0(qar@1.0.0)' as DepPath]: {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        ['/foo@1.1.0' as DepPath]: {
          resolution: {
            integrity: '',
          },
        },
        ['/qar@1.0.0' as DepPath]: {
          resolution: {
            integrity: '',
          },
        },
      },
    }
  )
  expect(mergedLockfile.packages?.['/a@1.0.0' as DepPath].dependencies?.linked).toBe('link:../1')
  expect(mergedLockfile.packages?.['/a@1.0.0' as DepPath].dependencies?.foo).toBe('1.2.0')
  expect(mergedLockfile.packages?.['/a@1.0.0' as DepPath].dependencies?.bar).toBe('4.0.0(qar@1.0.0)')
  expect(mergedLockfile.packages?.['/a@1.0.0' as DepPath].dependencies?.zoo).toBe('4.0.0(qar@1.0.0)')
  expect(Object.keys(mergedLockfile.packages ?? {}).sort()).toStrictEqual([
    '/a@1.0.0',
    '/bar@3.0.0(qar@1.0.0)',
    '/bar@4.0.0(qar@1.0.0)',
    '/foo@1.0.0',
    '/foo@1.1.0',
    '/foo@1.2.0',
    '/qar@1.0.0',
    '/zoo@3.0.0(qar@1.0.0)',
    '/zoo@4.0.0(qar@1.0.0)',
  ])
})

test('prefers our lockfile resolutions when it has newer packages', () => {
  const mergedLockfile = mergeLockfileChanges(
    {
      ...simpleLockfile,
      packages: {
        ['/foo@1.0.0' as DepPath]: {
          dependencies: {
            bar: '1.0.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        ['/bar@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
      },
    },
    {
      ...simpleLockfile,
      packages: {
        ['/foo@1.0.0' as DepPath]: {
          dependencies: {
            bar: '1.1.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        ['/bar@1.1.0' as DepPath]: {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        ['/qar@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
      },
    }
  )

  expect(mergedLockfile).toStrictEqual({
    ...simpleLockfile,
    packages: {
      '/foo@1.0.0': {
        dependencies: {
          bar: '1.1.0',
        },
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/bar@1.0.0': {
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/bar@1.1.0': {
        dependencies: {
          qar: '1.0.0',
        },
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/qar@1.0.0': {
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
    },
  })
})

test('prefers our lockfile resolutions when it has newer packages #2', () => {
  const mergedLockfile = mergeLockfileChanges(
    {
      ...simpleLockfile,
      packages: {
        ['/foo@1.0.0' as DepPath]: {
          dependencies: {
            bar: '1.0.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        ['/bar@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
      },
    },
    {
      ...simpleLockfile,
      packages: {
        ['/foo@1.0.0' as DepPath]: {
          dependencies: {
            bar: '1.1.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        ['/bar@1.1.0' as DepPath]: {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        ['/qar@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
      },
    }
  )

  expect(mergedLockfile).toStrictEqual({
    ...simpleLockfile,
    packages: {
      '/foo@1.0.0': {
        dependencies: {
          bar: '1.1.0',
        },
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/bar@1.0.0': {
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/bar@1.1.0': {
        dependencies: {
          qar: '1.0.0',
        },
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/qar@1.0.0': {
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
    },
  })
})

test('does not crash when merging non-semver versions (link: protocol)', () => {
  const base: LockfileObject = {
    importers: {
      ['.' as ProjectId]: {
        dependencies: { a: '1.0.0' },
        specifiers: {},
      },
    },
    lockfileVersion: '5.2',
    packages: {
      ['/a@1.0.0' as DepPath]: {
        dependencies: { linked: 'link:../pkg1' },
        resolution: { integrity: '' },
      },
    },
  }

  const mergedLockfile = mergeLockfileChanges(
    base,
    {
      ...base,
      packages: {
        ['/a@1.0.0' as DepPath]: {
          dependencies: { linked: 'link:../pkg2' },
          resolution: { integrity: '' },
        },
      },
    }
  )

  // Should not crash and should pick theirs (the incoming change)
  expect(mergedLockfile.packages?.['/a@1.0.0' as DepPath].dependencies?.linked).toBe('link:../pkg2')
})

test('preserves matching untracked pnpmfile hook state', () => {
  const lockfile = {
    ...simpleLockfile,
    untrackedPnpmfileReadPackageHook: false,
  }
  expect(mergeLockfileChanges(lockfile, lockfile).untrackedPnpmfileReadPackageHook).toBe(false)
})

test('marks conflicting untracked pnpmfile hook state for resolution', () => {
  expect(mergeLockfileChanges(
    {
      ...simpleLockfile,
      untrackedPnpmfileReadPackageHook: false,
    },
    {
      ...simpleLockfile,
      untrackedPnpmfileReadPackageHook: true,
    }
  ).untrackedPnpmfileReadPackageHook).toBe(true)
})

test('preserves and merges overrides, neverBuiltDependencies, patchedDependencies, settings, and catalogs', () => {
  const ours: LockfileObject = {
    ...simpleLockfile,
    overrides: {
      foo: '1.0.0',
      bar: '2.0.0',
    },
    neverBuiltDependencies: ['fsevents'],
    onlyBuiltDependencies: ['esbuild'],
    patchedDependencies: {
      'foo@1.0.0': 'hash-ours',
      'baz@3.0.0': 'hash-baz',
    },
    packageExtensionsChecksum: 'checksum-ours',
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
      dedupePeers: true,
    },
    catalogs: {
      default: {
        react: {
          specifier: '^18.0.0',
          version: '18.2.0',
        },
        lodash: {
          specifier: '^4.17.20',
          version: '4.17.21',
        },
      },
    },
    time: {
      foo: '2024-01-01T00:00:00.000Z',
    },
  }
  const theirs: LockfileObject = {
    ...simpleLockfile,
    overrides: {
      foo: '1.1.0',
      qar: '3.0.0',
    },
    neverBuiltDependencies: ['node-gyp'],
    onlyBuiltDependencies: ['sqlite3'],
    patchedDependencies: {
      'foo@1.0.0': 'hash-theirs',
      'qar@2.0.0': 'hash-qar',
    },
    packageExtensionsChecksum: 'checksum-theirs',
    settings: {
      autoInstallPeers: false,
      excludeLinksFromLockfile: true,
      peersSuffixMaxLength: 500,
    },
    catalogs: {
      default: {
        react: {
          specifier: '^18.3.0',
          version: '18.3.1',
        },
        axios: {
          specifier: '^1.0.0',
          version: '1.6.0',
        },
      },
      other: {
        vue: {
          specifier: '^3.0.0',
          version: '3.4.0',
        },
      },
    },
    time: {
      bar: '2024-02-01T00:00:00.000Z',
    },
  }

  const merged = mergeLockfileChanges(ours, theirs)

  expect(merged.overrides).toStrictEqual({
    foo: '1.1.0',
    bar: '2.0.0',
    qar: '3.0.0',
  })
  expect(merged.neverBuiltDependencies).toStrictEqual(['fsevents', 'node-gyp'])
  expect(merged.onlyBuiltDependencies).toStrictEqual(['esbuild', 'sqlite3'])
  expect(merged.patchedDependencies).toStrictEqual({
    'foo@1.0.0': 'hash-theirs',
    'baz@3.0.0': 'hash-baz',
    'qar@2.0.0': 'hash-qar',
  })
  expect(merged.packageExtensionsChecksum).toBe('checksum-ours')
  expect(merged.settings).toStrictEqual({
    autoInstallPeers: true,
    excludeLinksFromLockfile: true,
    dedupePeers: true,
    peersSuffixMaxLength: 500,
  })
  expect(merged.catalogs).toStrictEqual({
    default: {
      react: {
        specifier: '^18.3.0',
        version: '18.3.1',
      },
      lodash: {
        specifier: '^4.17.20',
        version: '4.17.21',
      },
      axios: {
        specifier: '^1.0.0',
        version: '1.6.0',
      },
    },
    other: {
      vue: {
        specifier: '^3.0.0',
        version: '3.4.0',
      },
    },
  })
  expect(merged.time).toStrictEqual({
    foo: '2024-01-01T00:00:00.000Z',
    bar: '2024-02-01T00:00:00.000Z',
  })
})

test('preserves foreign top-level keys', () => {
  const ours = {
    ...simpleLockfile,
    bit: { depsRequiringBuild: ['ours'] },
  } as unknown as LockfileObject
  const theirs = {
    ...simpleLockfile,
    bit: { depsRequiringBuild: ['theirs'] },
    otherTool: true,
  } as unknown as LockfileObject

  const merged = mergeLockfileChanges(ours, theirs) as unknown as Record<string, unknown>
  expect(merged.bit).toStrictEqual({ depsRequiringBuild: ['ours'] })
  expect(merged.otherTool).toBe(true)
})

test('preserves dependenciesMeta and publishDirectory of importers', () => {
  const ours: LockfileObject = {
    importers: {
      ['.' as ProjectId]: {
        dependencies: { foo: '1.0.0', bar: '1.0.0' },
        specifiers: { foo: '1.0.0', bar: '1.0.0' },
        dependenciesMeta: {
          foo: { injected: true },
          bar: { injected: false },
        },
        publishDirectory: 'dist',
      },
    },
    lockfileVersion: '6.0',
  }

  const theirs: LockfileObject = {
    importers: {
      ['.' as ProjectId]: {
        dependencies: { foo: '1.1.0', bar: '1.0.0', baz: '2.0.0' },
        specifiers: { foo: '1.1.0', bar: '1.0.0', baz: '2.0.0' },
        dependenciesMeta: {
          bar: { injected: true, patch: 'bar.patch' },
          baz: { injected: true },
        },
      },
    },
    lockfileVersion: '6.0',
  }

  const mergedLockfile = mergeLockfileChanges(ours, theirs)

  expect(mergedLockfile.importers['.' as ProjectId].dependenciesMeta).toStrictEqual({
    foo: { injected: true },
    bar: { injected: true, patch: 'bar.patch' },
    baz: { injected: true },
  })
  expect(mergedLockfile.importers['.' as ProjectId].publishDirectory).toBe('dist')
})
