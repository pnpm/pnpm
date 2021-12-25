import path from 'path'
import hoist from '@pnpm/real-hoist'
import { readWantedLockfile } from '@pnpm/lockfile-file'

test('hoist', async () => {
  const lockfile = await readWantedLockfile(path.join(__dirname, '../../..'), { ignoreIncompatible: true })
  expect(hoist(lockfile!)).toBeTruthy()
})
