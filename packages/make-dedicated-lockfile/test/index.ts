import path from 'path'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import fixtures from '@pnpm/test-fixtures'
import makeDedicatedLockfile from '../lib'

const f = fixtures(__dirname)

test('makeDedicatedLockfile()', async () => {
  const tmp = f.prepare('fixture')
  const projectDir = path.join(tmp, 'packages/is-negative')
  await makeDedicatedLockfile(tmp, projectDir)

  const lockfile = await readWantedLockfile(projectDir, { ignoreIncompatible: false })
  expect(Object.keys(lockfile?.importers ?? {})).toStrictEqual(['.', 'example'])
  expect(Object.keys(lockfile?.packages ?? {})).toStrictEqual([
    '/is-positive/1.0.0',
    '/lodash/1.0.0',
    '/ramda/0.26.0',
  ])
})
