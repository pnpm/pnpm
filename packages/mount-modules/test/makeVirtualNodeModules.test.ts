import path from 'path'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import makeVirtualNodeModules from '../src/makeVirtualNodeModules'

test('makeVirtualNodeModules', async () => {
  const lockfile = await readWantedLockfile(path.join(__dirname, '__fixtures__/simple'), { ignoreIncompatible: true })
  expect(makeVirtualNodeModules(lockfile!)).toMatchSnapshot()
})
