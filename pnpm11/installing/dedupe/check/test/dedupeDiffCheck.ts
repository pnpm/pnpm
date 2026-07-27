import { describe, expect, it } from '@jest/globals'
import {
  calcDedupeCheckIssues,
  DedupeCheckIssuesError,
  dedupeDiffCheck,
} from '@pnpm/installing.dedupe.check'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'

describe('dedupeDiffCheck', () => {
  it('should have no changes for same lockfile', () => {
    const lockfile: LockfileObject = {
      importers: {
        ['.' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: 'testLockfileVersion',
    }

    expect(() => {
      dedupeDiffCheck(lockfile, lockfile)
    }).not.toThrow()
  })

  it('throws DedupeCheckIssuesError on changes', () => {
    const before: LockfileObject = {
      importers: {
        ['packages/a' as ProjectId]: {
          specifiers: {
            'is-positive': '^3.0.0',
          },
          dependencies: {
            'is-positive': '3.0.0',
          },
        },
        ['packages/b' as ProjectId]: {
          specifiers: {
            'is-positive': '^3.1.0',
          },
          dependencies: {
            'is-positive': '3.1.0',
          },
        },
      },
      packages: {
        ['is-positive@3.0.0' as DepPath]: {
          resolution: {
            integrity: 'sha512-JDkaKp5jWv24ZaFuYDKTcBrC/wBOHdjhzLDkgrrkJD/j7KqqXsGcAkex336qHoOFEajMy7bYqUgm0KH9/MzQvw==',
          },
          engines: {
            node: '>=0.10.0',
          },
        },
        ['is-positive@3.1.0' as DepPath]: {
          resolution: {
            integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
          },
          engines: {
            node: '>=0.10.0',
          },
        },
      },
      lockfileVersion: 'testLockfileVersion',
    }

    const after: LockfileObject = {
      importers: {
        ['packages/a' as ProjectId]: {
          specifiers: {
            'is-positive': '^3.0.0',
          },
          dependencies: {
            'is-positive': '3.1.0',
          },
        },
        ['packages/b' as ProjectId]: {
          specifiers: {
            'is-positive': '^3.1.0',
          },
          dependencies: {
            'is-positive': '3.1.0',
          },
        },
      },
      packages: {
        ['is-positive@3.1.0' as DepPath]: {
          resolution: {
            integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
          },
          engines: {
            node: '>=0.10.0',
          },
        },
      },
      lockfileVersion: 'testLockfileVersion',
    }

    expect(() => {
      dedupeDiffCheck(before, after)
    }).toThrow(new DedupeCheckIssuesError({
      importerIssuesByImporterId: {
        added: [],
        removed: [],
        updated: {
          'packages/a': {
            'is-positive': { type: 'updated', prev: '3.0.0', next: '3.1.0' },
          },
        },
      },
      packageIssuesByDepPath: {
        added: [],
        removed: ['/is-positive@3.0.0'],
        updated: {},
      },
    }))
  })

  it('sorts snapshot changes independently of resolution order', () => {
    const before: LockfileObject = {
      importers: {
        ['.' as ProjectId]: {
          specifiers: {},
        },
      },
      packages: {
        ['z@1.0.0' as DepPath]: {
          resolution: { integrity: 'sha512-z' },
        },
        ['a@1.0.0' as DepPath]: {
          resolution: { integrity: 'sha512-a' },
          dependencies: {
            dependency: '1.0.0',
          },
        },
      },
      lockfileVersion: 'testLockfileVersion',
    }
    const after: LockfileObject = {
      importers: {
        ['.' as ProjectId]: {
          specifiers: {},
        },
      },
      packages: {
        ['z-new@1.0.0' as DepPath]: {
          resolution: { integrity: 'sha512-z-new' },
        },
        ['a-new@1.0.0' as DepPath]: {
          resolution: { integrity: 'sha512-a-new' },
        },
        ['z@1.0.0' as DepPath]: {
          resolution: { integrity: 'sha512-z' },
          dependencies: {
            dependency: '2.0.0',
          },
        },
        ['a@1.0.0' as DepPath]: {
          resolution: { integrity: 'sha512-a' },
          dependencies: {
            dependency: '2.0.0',
          },
        },
      },
      lockfileVersion: 'testLockfileVersion',
    }

    expect(calcDedupeCheckIssues(before, after).packageIssuesByDepPath).toEqual({
      added: ['a-new@1.0.0', 'z-new@1.0.0'],
      removed: [],
      updated: {
        'a@1.0.0': {
          dependency: { type: 'updated', prev: '1.0.0', next: '2.0.0' },
        },
        'z@1.0.0': {
          dependency: { type: 'added', next: '2.0.0' },
        },
      },
    })
  })
})
