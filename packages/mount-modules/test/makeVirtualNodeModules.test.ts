import path from 'path'
import { fileURLToPath } from 'url'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import makeVirtualNodeModules from '../src/makeVirtualNodeModules'

const dirname = path.dirname(fileURLToPath(import.meta.url))

test('makeVirtualNodeModules', async () => {
  const lockfile = await readWantedLockfile(path.join(dirname, '__fixtures__/simple'), { ignoreIncompatible: true })
  const cafsDir = path.join(__dirname, '__fixtures__/simple/store/v3/files')
  expect(makeVirtualNodeModules(lockfile!, cafsDir)).toMatchSnapshot()
})
