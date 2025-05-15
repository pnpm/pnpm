import { hoist } from '@pnpm/real-hoist'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { fixtures } from '@pnpm/test-fixtures'
import { type ProjectId } from '@pnpm/types'

const f = fixtures(__dirname)

test('hoist', async () => {
  const lockfile = await readWantedLockfile(f.find('fixture'), { ignoreIncompatible: true })
  expect(hoist(lockfile!)).toBeTruthy()
})

test('hoist throws an error if the lockfile is broken', () => {
  expect(() => hoist({
    lockfileVersion: '5',
    importers: {
      ['.' as ProjectId]: {
        dependencies: {
          foo: '1.0.0',
        },
        specifiers: {
          foo: '1.0.0',
        },
      },
    },
    packages: {},
  })).toThrow(/Broken lockfile/)
})
