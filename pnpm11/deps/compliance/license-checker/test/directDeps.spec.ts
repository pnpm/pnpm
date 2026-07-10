import { describe, expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.fs'

import { collectDirectDepKeys } from '../src/directDeps.js'

// The `dependencies`/`devDependencies`/`optionalDependencies` maps on a
// `LockfileObject` importer are `Record<alias, resolvedRef>` — a bare
// version string for a same-name dependency, or a "name@version" ref for an
// `npm:` alias (see `@pnpm/lockfile.types#ResolvedDependencies`). This is
// distinct from the on-disk lockfile *file* format, which nests
// `{ specifier, version }` per dependency; `@pnpm/lockfile.fs` flattens that
// into `specifiers` + `dependencies` when it reads the file.
const lockfile = {
  lockfileVersion: '9.0',
  importers: {
    '.': {
      specifiers: {
        positive: 'npm:is-positive@1.0.0',
        rimraf: '^5.0.0',
      },
      dependencies: {
        positive: 'is-positive@1.0.0',
      },
      devDependencies: {
        rimraf: '5.0.10',
      },
    },
    'packages/b': {
      specifiers: {
        'is-odd': '^3.0.0',
      },
      dependencies: {
        'is-odd': '3.0.1',
      },
    },
  },
  packages: {},
} as unknown as LockfileObject

describe('collectDirectDepKeys', () => {
  test('resolves aliased direct deps to their real name@version', () => {
    const keys = collectDirectDepKeys(lockfile)
    expect(keys.has('is-positive@1.0.0')).toBe(true) // alias resolved
    expect(keys.has('positive@1.0.0')).toBe(false) // alias key NOT used
    expect(keys.has('rimraf@5.0.10')).toBe(true)
  })

  test('no-arg call collects deps from every importer', () => {
    const keys = collectDirectDepKeys(lockfile)
    expect(keys.has('is-positive@1.0.0')).toBe(true) // from '.'
    expect(keys.has('rimraf@5.0.10')).toBe(true) // from '.'
    expect(keys.has('is-odd@3.0.1')).toBe(true) // from 'packages/b'
  })

  test('scoped to selected importers excludes other importers', () => {
    const keys = collectDirectDepKeys(lockfile, ['.'])
    // Root importer's deps are included ...
    expect(keys.has('is-positive@1.0.0')).toBe(true)
    expect(keys.has('rimraf@5.0.10')).toBe(true)
    // ... but the sibling importer's dep is excluded.
    expect(keys.has('is-odd@3.0.1')).toBe(false)
    expect(keys.size).toBe(2)
  })

  test('git/tarball direct dep resolves to the scanner-matching name@semver, not the URL', () => {
    // Mirrors pnpm11/__fixtures__/with-git-protocol-dep/pnpm-lock.yaml: the
    // importer ref is a git tarball URL, and the packages snapshot carries the
    // real semver in `version` (which is exactly what the license-scanner's
    // nameVerFromPkgSnapshot reports the package under).
    const gitUrl = 'https://codeload.github.com/kevva/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c'
    const gitDepPath = `is-negative@${gitUrl}`
    const gitLockfile = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          dependencies: {
            'is-negative': gitUrl,
          },
        },
      },
      packages: {
        [gitDepPath]: {
          version: '2.1.0',
          resolution: { gitHosted: true, tarball: gitUrl },
        },
      },
    } as unknown as LockfileObject
    const keys = collectDirectDepKeys(gitLockfile)
    expect(keys.has('is-negative@2.1.0')).toBe(true) // scanner-matching form
    expect(keys.has(gitDepPath)).toBe(false) // NOT the URL form
  })

  test('ignores workspace link: refs without crashing', () => {
    const withLink = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          dependencies: {
            positive: 'is-positive@1.0.0',
            sibling: 'link:../sibling',
          },
        },
      },
      packages: {},
    } as unknown as LockfileObject
    const keys = collectDirectDepKeys(withLink)
    expect(keys.has('is-positive@1.0.0')).toBe(true)
    expect([...keys].some((key) => key.startsWith('sibling'))).toBe(false)
  })

  test('returns no keys for an unknown importer id', () => {
    const keys = collectDirectDepKeys(lockfile, ['does-not-exist'])
    expect(keys.size).toBe(0)
  })
})
