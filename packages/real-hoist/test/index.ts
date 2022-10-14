import path from 'path'
import { hoist } from '@pnpm/real-hoist'
import { readWantedLockfile } from '@pnpm/lockfile-file'

test('hoist', async () => {
  const lockfile = await readWantedLockfile(path.join(__dirname, '../../..'), { ignoreIncompatible: true })
  expect(hoist(lockfile!)).toBeTruthy()
})

test('hoist throws an error if the lockfile is broken', () => {
  expect(() => hoist({
    lockfileVersion: 5,
    importers: {
      '.': {
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
