import { expect, test } from '@jest/globals'
import type { LockfileFile } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'
import yaml from 'js-yaml'

import { convertToLockfileFile, convertToLockfileObject } from '../lib/lockfileFormatConverters.js'

test('convertToLockfileFile()', () => {
  const lockfileV5 = {
    lockfileVersion: '9.0',
    importers: {
      project1: {
        specifiers: {
          foo: '^1.0.0',
          bar: '^1.0.0',
          qar: '^1.0.0',
          tarball: '^1.0.0',
        },
        dependencies: {
          foo: '1.0.0',
          tarball: '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        },
        devDependencies: {
          bar: '/@bar/bar@1.0.0(@babel/core@2.0.0)',
        },
        optionalDependencies: {
          qar: 'reg.com/qar@1.0.0',
        },
      },
    },
    packages: {
      '/foo@1.0.0': {
        resolution: { integrity: '' },
      },
      '/@bar/bar@1.0.0(@babel/core@2.0.0)': {
        resolution: { integrity: '' },
      },
      'reg.com/qar@1.0.0': {
        resolution: { integrity: '' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        resolution: { integrity: '' },
      },
    },
  }
  const lockfileV6 = {
    lockfileVersion: '9.0',
    importers: {
      project1: {
        dependencies: {
          foo: {
            specifier: '^1.0.0',
            version: '1.0.0',
          },
          tarball: {
            specifier: '^1.0.0',
            version: '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          },
        },
        devDependencies: {
          bar: {
            specifier: '^1.0.0',
            version: '/@bar/bar@1.0.0(@babel/core@2.0.0)',
          },
        },
        optionalDependencies: {
          qar: {
            specifier: '^1.0.0',
            version: 'reg.com/qar@1.0.0',
          },
        },
      },
    },
    packages: {
      '/foo@1.0.0': {
        resolution: { integrity: '' },
      },
      '/@bar/bar@1.0.0': {
        resolution: { integrity: '' },
      },
      'reg.com/qar@1.0.0': {
        resolution: { integrity: '' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        resolution: { integrity: '' },
      },
    },
    snapshots: {
      '/foo@1.0.0': {},
      '/@bar/bar@1.0.0(@babel/core@2.0.0)': {},
      'reg.com/qar@1.0.0': {},
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {},
    },
  }
  expect(convertToLockfileFile(lockfileV5)).toEqual(lockfileV6)
  expect(convertToLockfileObject(lockfileV6)).toEqual(lockfileV5)
})

test('convertToLockfileObject() reconstructs a dropped directory resolution for a pruned file: peer-variant, but never for a file: tarball', () => {
  // Simulates a pruned lockfile (e.g. after `turbo prune --docker`): the
  // base `pkg@file:...` packages entry that carried `resolution` is gone,
  // only the peer-variant snapshot remains.
  const prunedLockfileV6 = {
    lockfileVersion: '9.0',
    importers: {},
    snapshots: {
      'dir@file:packages/dir(peer@1.0.0)': {},
      'tar@file:vendor/tar-1.0.0.tgz(peer@1.0.0)': {},
      // Uppercase tarball extensions must be treated as tarballs too — the
      // resolver in resolving/local-resolver/src/parseBareSpecifier.ts
      // matches /\.(?:tgz|tar.gz|tar)$/i, so the boundary applied here at
      // load time has to be case-insensitive in lockstep.
      'upper@file:vendor/upper-1.0.0.TGZ(peer@1.0.0)': {},
      'mixed@file:vendor/mixed-1.0.0.Tar.Gz(peer@1.0.0)': {},
    },
  }
  const lockfile = convertToLockfileObject(prunedLockfileV6)
  // Local-directory `file:` ref → directory resolution losslessly reconstructed.
  expect(lockfile.packages?.['dir@file:packages/dir(peer@1.0.0)' as DepPath]?.resolution).toEqual({
    directory: 'packages/dir',
    type: 'directory',
  })
  // `file:` tarball ref → must NOT be turned into a directory resolution.
  expect(lockfile.packages?.['tar@file:vendor/tar-1.0.0.tgz(peer@1.0.0)' as DepPath]?.resolution).toBeUndefined()
  expect(lockfile.packages?.['upper@file:vendor/upper-1.0.0.TGZ(peer@1.0.0)' as DepPath]?.resolution).toBeUndefined()
  expect(lockfile.packages?.['mixed@file:vendor/mixed-1.0.0.Tar.Gz(peer@1.0.0)' as DepPath]?.resolution).toBeUndefined()
})

test('convertToLockfileFile() with lockfile v6', () => {
  const lockfileV5 = {
    lockfileVersion: '9.0',
    importers: {
      project1: {
        specifiers: {
          foo: '^1.0.0',
          bar: '^1.0.0',
          qar: '^1.0.0',
          tarball: '^1.0.0',
        },
        dependencies: {
          foo: '1.0.0',
          tarball: '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        },
        devDependencies: {
          bar: '/@bar/bar@1.0.0(@babel/core@2.0.0)',
        },
        optionalDependencies: {
          qar: 'reg.com/qar@1.0.0',
        },
      },
    },
    packages: {
      '/foo@1.0.0': {
        resolution: { integrity: '' },
      },
      '/@bar/bar@1.0.0(@babel/core@2.0.0)': {
        resolution: { integrity: '' },
      },
      'reg.com/qar@1.0.0': {
        resolution: { integrity: '' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        resolution: { integrity: '' },
      },
    },
  }
  const lockfileV6 = {
    lockfileVersion: '9.0',
    importers: {
      project1: {
        dependencies: {
          foo: {
            specifier: '^1.0.0',
            version: '1.0.0',
          },
          tarball: {
            specifier: '^1.0.0',
            version: '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          },
        },
        devDependencies: {
          bar: {
            specifier: '^1.0.0',
            version: '/@bar/bar@1.0.0(@babel/core@2.0.0)',
          },
        },
        optionalDependencies: {
          qar: {
            specifier: '^1.0.0',
            version: 'reg.com/qar@1.0.0',
          },
        },
      },
    },
    packages: {
      '/foo@1.0.0': {
        resolution: { integrity: '' },
      },
      '/@bar/bar@1.0.0': {
        resolution: { integrity: '' },
      },
      'reg.com/qar@1.0.0': {
        resolution: { integrity: '' },
      },
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {
        resolution: { integrity: '' },
      },
    },
    snapshots: {
      '/foo@1.0.0': {},
      '/@bar/bar@1.0.0(@babel/core@2.0.0)': {},
      'reg.com/qar@1.0.0': {},
      '@registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz': {},
    },
  }
  expect(convertToLockfileFile(lockfileV5)).toEqual(lockfileV6)
  expect(convertToLockfileObject(lockfileV6)).toEqual(lockfileV5)
})

test('convertToLockfileObject() reads a dependency named constructor', () => {
  const lockfileFile = yaml.load(`
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      constructor:
        specifier: ^0.0.6
        version: 0.0.6
packages:
  constructor@0.0.6:
    resolution: {integrity: sha512-constructor}
snapshots:
  constructor@0.0.6: {}
`) as LockfileFile
  const lockfile = convertToLockfileObject(lockfileFile)
  expect(lockfile.importers['.' as ProjectId]).toStrictEqual({
    specifiers: { constructor: '^0.0.6' },
    dependencies: { constructor: '0.0.6' },
    devDependencies: undefined,
    optionalDependencies: undefined,
  })
  expect(lockfile.packages?.['constructor@0.0.6' as DepPath]).toStrictEqual({
    resolution: { integrity: 'sha512-constructor' },
  })
  expect(convertToLockfileFile(lockfile).importers?.['.']).toEqual({
    dependencies: {
      constructor: { specifier: '^0.0.6', version: '0.0.6' },
    },
  })
})

test('convertToLockfileObject() keeps __proto__ keys of a lockfile as own properties', () => {
  const lockfileFile = yaml.load(`
lockfileVersion: '9.0'
importers:
  __proto__:
    dependencies:
      __proto__:
        specifier: ^1.0.0
        version: 1.0.0
packages:
  foo@1.0.0:
    resolution: {integrity: sha512-foo}
    __proto__:
      polluted: true
snapshots:
  foo@1.0.0: {}
  __proto__:
    dependencies:
      foo: 1.0.0
patchedDependencies:
  __proto__:
    hash: abc
`) as LockfileFile
  const lockfile = convertToLockfileObject(lockfileFile)

  expect(({} as Record<string, unknown>).polluted).toBeUndefined()
  expect(Object.prototype).not.toHaveProperty('polluted')

  const foo = lockfile.packages!['foo@1.0.0' as DepPath]
  expect(Object.getPrototypeOf(foo)).toBe(Object.prototype)
  expect(foo).not.toHaveProperty('polluted')
  expect(Object.getOwnPropertyDescriptor(foo, '__proto__')?.value).toStrictEqual({ polluted: true })
  expect(foo.resolution).toStrictEqual({ integrity: 'sha512-foo' })

  expect(Object.getPrototypeOf(lockfile.packages)).toBe(Object.prototype)
  expect(Object.keys(lockfile.packages!)).toStrictEqual(['foo@1.0.0', '__proto__'])
  expect(Object.getOwnPropertyDescriptor(lockfile.packages, '__proto__')?.value).toStrictEqual({
    dependencies: { foo: '1.0.0' },
  })

  expect(Object.getPrototypeOf(lockfile.importers)).toBe(Object.prototype)
  expect(Object.keys(lockfile.importers)).toStrictEqual(['__proto__'])
  const importer = Object.getOwnPropertyDescriptor(lockfile.importers, '__proto__')?.value
  expect(Object.getPrototypeOf(importer.specifiers)).toBe(Object.prototype)
  expect(Object.getOwnPropertyDescriptor(importer.specifiers, '__proto__')?.value).toBe('^1.0.0')
  expect(Object.getPrototypeOf(importer.dependencies)).toBe(Object.prototype)
  expect(Object.getOwnPropertyDescriptor(importer.dependencies, '__proto__')?.value).toBe('1.0.0')

  expect(Object.getPrototypeOf(lockfile.patchedDependencies)).toBe(Object.prototype)
  expect(Object.getOwnPropertyDescriptor(lockfile.patchedDependencies, '__proto__')?.value).toBe('abc')

  const lockfileFileAgain = convertToLockfileFile(lockfile)
  expect(({} as Record<string, unknown>).polluted).toBeUndefined()
  expect(Object.keys(lockfileFileAgain.importers!)).toStrictEqual(['__proto__'])
  expect(Object.keys(lockfileFileAgain.packages!)).toStrictEqual(['foo@1.0.0', '__proto__'])
  expect(Object.keys(lockfileFileAgain.snapshots!)).toStrictEqual(['foo@1.0.0', '__proto__'])
  expect(Object.getOwnPropertyDescriptor(lockfileFileAgain.snapshots, '__proto__')?.value).toEqual({
    dependencies: { foo: '1.0.0' },
  })
})
