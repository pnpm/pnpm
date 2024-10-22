import { LOCKFILE_VERSION } from '@pnpm/constants'
import { type Lockfile } from '@pnpm/lockfile.fs'
import { type ProjectId } from '@pnpm/types'
import { assertLockfilesEqual } from '../src/assertLockfilesEqual'

const wantedLockfile: Lockfile = {
  lockfileVersion: LOCKFILE_VERSION,
  importers: {
    ['.' as ProjectId]: {
      specifiers: {
        foo: '^1.0.0',
      },
      dependencies: {
        foo: '1.0.1',
      },
    },
  },
}

test.skip('should not error if the currentLockfile does not exist', () => {
  assertLockfilesEqual(null, wantedLockfile, '<LOCKFILE_DIR>')
})

test.todo('more test cases')
