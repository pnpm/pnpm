import { expect, test } from '@jest/globals'
import type { DepPath } from '@pnpm/types'

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
