import { readWantedLockfile } from '@pnpm/lockfile-file'
import makeVirtualNodeModules from '../src/makeVirtualNodeModules'
import path = require('path')

test('makeVirtualNodeModules', async () => {
  const lockfile = await readWantedLockfile(path.join(__dirname, '__fixtures__/simple'), { ignoreIncompatible: true })
  const cafsDir = path.join(__dirname, '__fixtures__/simple/store/v3/files')
  expect(makeVirtualNodeModules(lockfile!, cafsDir)).toMatchSnapshot()
})
