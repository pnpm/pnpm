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

  test('scoped to selected importers', () => {
    const keys = collectDirectDepKeys(lockfile, ['.'])
    expect(keys.size).toBe(2)
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
