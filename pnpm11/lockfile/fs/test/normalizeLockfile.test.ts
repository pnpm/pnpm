import { expect, test } from '@jest/globals'
import { LOCKFILE_VERSION, NAMED_REGISTRIES_LOCKFILE_VERSION } from '@pnpm/constants'
import type { DepPath, ProjectId } from '@pnpm/types'

import { convertToLockfileFile } from '../lib/lockfileFormatConverters.js'

test('empty overrides are removed during lockfile normalization', () => {
  expect(convertToLockfileFile({
    lockfileVersion: LOCKFILE_VERSION,
    overrides: {},
    patchedDependencies: {},
    packages: {},
    importers: {
      ['foo' as ProjectId]: {
        dependencies: {
          bar: 'link:../bar',
        },
        specifiers: {
          bar: 'link:../bar',
        },
      },
    },
  })).toStrictEqual({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      foo: {
        dependencies: {
          bar: {
            version: 'link:../bar',
            specifier: 'link:../bar',
          },
        },
      },
    },
  })
})

test('redundant fields are removed from "time"', () => {
  expect(convertToLockfileFile({
    lockfileVersion: LOCKFILE_VERSION,
    packages: {},
    importers: {
      ['foo' as ProjectId]: {
        dependencies: {
          bar: '1.0.0',
        },
        devDependencies: {
          foo: '1.0.0(react@18.0.0)',
        },
        optionalDependencies: {
          qar: '1.0.0',
        },
        specifiers: {
          bar: '1.0.0',
          foo: '1.0.0',
          qar: '1.0.0',
        },
      },
    },
    time: {
      'bar@1.0.0': '2021-02-11T22:54:29.120Z',
      'foo@1.0.0': '2021-02-11T22:54:29.120Z',
      'qar@1.0.0': '2021-02-11T22:54:29.120Z',
      'zoo@1.0.0': '2021-02-11T22:54:29.120Z',
    },
  })).toStrictEqual({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      foo: {
        dependencies: {
          bar: {
            version: '1.0.0',
            specifier: '1.0.0',
          },
        },
        devDependencies: {
          foo: {
            version: '1.0.0(react@18.0.0)',
            specifier: '1.0.0',
          },
        },
        optionalDependencies: {
          qar: {
            version: '1.0.0',
            specifier: '1.0.0',
          },
        },
      },
    },
    time: {
      'bar@1.0.0': '2021-02-11T22:54:29.120Z',
      'foo@1.0.0': '2021-02-11T22:54:29.120Z',
      'qar@1.0.0': '2021-02-11T22:54:29.120Z',
    },
  })
})

test('a registry-qualified package key stamps 12.0 and an existing 12.0 version is sticky', () => {
  const importers = {
    ['.' as ProjectId]: {
      dependencies: {
        foo: 'work:1.0.0',
      },
      specifiers: {
        foo: 'work:^1.0.0',
      },
    },
  }
  const withQualifiedKey = convertToLockfileFile({
    lockfileVersion: LOCKFILE_VERSION,
    importers,
    packages: {
      ['foo@work:1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-AAAA' },
      },
    },
  })
  expect(withQualifiedKey.lockfileVersion).toBe(NAMED_REGISTRIES_LOCKFILE_VERSION)

  const withoutQualifiedKey = convertToLockfileFile({
    lockfileVersion: NAMED_REGISTRIES_LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
    packages: {
      ['foo@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-AAAA' },
      },
    },
  })
  expect(withoutQualifiedKey.lockfileVersion).toBe(NAMED_REGISTRIES_LOCKFILE_VERSION)
})
