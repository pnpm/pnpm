import { LOCKFILE_VERSION } from '@pnpm/constants'
import { sortLockfileKeys } from '../lib/sortLockfileKeys'

test('sorts keys alphabetically', () => {
  const normalizedLockfile = sortLockfileKeys({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      foo: {
        dependencies: {
          zzz: 'link:../zzz',
          bar: 'link:../bar',
          aaa: 'link:../aaa',
        },
        specifiers: {
          zzz: 'link:../zzz',
          bar: 'link:../bar',
          aaa: 'link:../aaa',
        },
      },
      bar: {
        specifiers: {
          baz: 'link:../baz',
        },
        dependencies: {
          baz: 'link:../baz',
        },
      },
    },
  })

  expect(normalizedLockfile).toStrictEqual({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      bar: {
        dependencies: {
          baz: 'link:../baz',
        },
        specifiers: {
          baz: 'link:../baz',
        },
      },
      foo: {
        dependencies: {
          aaa: 'link:../aaa',
          bar: 'link:../bar',
          zzz: 'link:../zzz',
        },
        specifiers: {
          aaa: 'link:../aaa',
          bar: 'link:../bar',
          zzz: 'link:../zzz',
        },
      },
    },
  })
  expect(Object.keys(normalizedLockfile.importers?.foo.dependencies ?? {})).toStrictEqual(['aaa', 'bar', 'zzz'])
  expect(Object.keys(normalizedLockfile.importers?.foo.specifiers ?? {})).toStrictEqual(['aaa', 'bar', 'zzz'])
})

test('sorting does not care about locale (e.g. Czech has "ch" as a single character after "h")', () => {
  // The input is properly sorted according to Czech locale.
  const normalizedLockfile = sortLockfileKeys({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      foo: {
        dependencies: {
          bar: 'link:../bar',
          href: 'link:../href',
          chmod: 'link:../chmod',
        },
        specifiers: {
          bar: 'link:../bar',
          href: 'link:../href',
          chmod: 'link:../chmod',
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
          bar: 'link:../bar',
          chmod: 'link:../chmod',
          href: 'link:../href',
        },
        specifiers: {
          bar: 'link:../bar',
          chmod: 'link:../chmod',
          href: 'link:../href',
        },
      },
    },
  })
  expect(Object.keys(normalizedLockfile.importers?.foo.dependencies ?? {})).toStrictEqual(['bar', 'chmod', 'href'])
  expect(Object.keys(normalizedLockfile.importers?.foo.specifiers ?? {})).toStrictEqual(['bar', 'chmod', 'href'])
})
