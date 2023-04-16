import { DedupeCheckIssuesError, dedupeDiffCheck } from '@pnpm/dedupe.check'
import { type Lockfile } from '@pnpm/lockfile-types'

describe('dedupeDiffCheck', () => {
  it('should have no changes for same lockfile', () => {
    const lockfile: Lockfile = {
      importers: {
        '.': {
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
    const before: Lockfile = {
      importers: {
        'packages/a': {
          specifiers: {
            'is-positive': '^3.0.0',
          },
          dependencies: {
            'is-positive': '3.0.0',
          },
        },
        'packages/b': {
          specifiers: {
            'is-positive': '^3.1.0',
          },
          dependencies: {
            'is-positive': '3.1.0',
          },
        },
      },
      packages: {
        '/is-positive@3.0.0': {
          resolution: {
            integrity: 'sha512-JDkaKp5jWv24ZaFuYDKTcBrC/wBOHdjhzLDkgrrkJD/j7KqqXsGcAkex336qHoOFEajMy7bYqUgm0KH9/MzQvw==',
          },
          engines: {
            node: '>=0.10.0',
          },
        },
        '/is-positive@3.1.0': {
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

    const after: Lockfile = {
      importers: {
        'packages/a': {
          specifiers: {
            'is-positive': '^3.0.0',
          },
          dependencies: {
            'is-positive': '3.1.0',
          },
        },
        'packages/b': {
          specifiers: {
            'is-positive': '^3.1.0',
          },
          dependencies: {
            'is-positive': '3.1.0',
          },
        },
      },
      packages: {
        '/is-positive@3.1.0': {
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
})
