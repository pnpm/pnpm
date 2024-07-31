import { LOCKFILE_VERSION } from '@pnpm/constants'
import { sortLockfileKeys } from '../lib/sortLockfileKeys'

test('sorts keys alphabetically', () => {
  const normalizedLockfile = sortLockfileKeys({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      foo: {
        dependencies: {
          zzz: {
            version: 'link:../zzz',
            specifier: 'link:../zzz',
          },
          bar: {
            version: 'link:../bar',
            specifier: 'link:../bar',
          },
          aaa: {
            version: 'link:../aaa',
            specifier: 'link:../aaa',
          },
        },
      },
      bar: {
        dependencies: {
          baz: {
            version: 'link:../baz',
            specifier: 'link:../baz',
          },
        },
      },
    },
    patchedDependencies: {
      zzz: { path: 'foo', hash: 'bar' },
      bar: { path: 'foo', hash: 'bar' },
      aaa: { path: 'foo', hash: 'bar' },
    },
  })

  expect(normalizedLockfile).toStrictEqual({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      bar: {
        dependencies: {
          baz: {
            version: 'link:../baz',
            specifier: 'link:../baz',
          },
        },
      },
      foo: {
        dependencies: {
          aaa: {
            version: 'link:../aaa',
            specifier: 'link:../aaa',
          },
          bar: {
            version: 'link:../bar',
            specifier: 'link:../bar',
          },
          zzz: {
            version: 'link:../zzz',
            specifier: 'link:../zzz',
          },
        },
      },
    },
    patchedDependencies: {
      aaa: { path: 'foo', hash: 'bar' },
      bar: { path: 'foo', hash: 'bar' },
      zzz: { path: 'foo', hash: 'bar' },
    },
  })
  expect(Object.keys(normalizedLockfile.importers?.foo.dependencies ?? {})).toStrictEqual(['aaa', 'bar', 'zzz'])
  expect(Object.keys(normalizedLockfile.patchedDependencies ?? {})).toStrictEqual(['aaa', 'bar', 'zzz'])
})

test('sorting does not care about locale (e.g. Czech has "ch" as a single character after "h")', () => {
  // The input is properly sorted according to Czech locale.
  const normalizedLockfile = sortLockfileKeys({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      foo: {
        dependencies: {
          bar: {
            version: 'link:../bar',
            specifier: 'link:../bar',
          },
          href: {
            version: 'link:../href',
            specifier: 'link:../href',
          },
          chmod: {
            version: 'link:../chmod',
            specifier: 'link:../chmod',
          },
        },
      },
    },
  })

  // The result should be the same as on other machines using whatever locale, e.g. English.
  expect(normalizedLockfile).toStrictEqual({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      foo: {
        dependencies: {
          bar: {
            version: 'link:../bar',
            specifier: 'link:../bar',
          },
          chmod: {
            version: 'link:../chmod',
            specifier: 'link:../chmod',
          },
          href: {
            version: 'link:../href',
            specifier: 'link:../href',
          },
        },
      },
    },
  })
  expect(Object.keys(normalizedLockfile.importers?.foo.dependencies ?? {})).toStrictEqual(['bar', 'chmod', 'href'])
})
