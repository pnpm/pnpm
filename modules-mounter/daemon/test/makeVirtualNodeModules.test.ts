import path from 'path'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { makeVirtualNodeModules } from '../src/makeVirtualNodeModules.js'

test('makeVirtualNodeModules', async () => {
  const lockfile = await readWantedLockfile(path.join(import.meta.dirname, '__fixtures__/simple'), { ignoreIncompatible: true })
  expect(makeVirtualNodeModules(lockfile!)).toMatchSnapshot()
})
