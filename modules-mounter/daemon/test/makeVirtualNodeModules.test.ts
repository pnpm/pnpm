import path from 'node:path'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { makeVirtualNodeModules } from '../src/makeVirtualNodeModules'

test('makeVirtualNodeModules', async () => {
  const lockfile = await readWantedLockfile(
    path.join(__dirname, '__fixtures__/simple'),
    { ignoreIncompatible: true }
  )
  expect(lockfile).not.toBeNull()
  // @ts-ignore
  expect(makeVirtualNodeModules(lockfile)).toMatchSnapshot()
})
