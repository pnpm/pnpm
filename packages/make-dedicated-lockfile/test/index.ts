import { promisify } from 'util'
import path from 'path'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { tempDir } from '@pnpm/prepare'
import ncpCB from 'ncp'
import makeDedicatedLockfile from '../lib'

const ncp = promisify(ncpCB)

const fixture = path.join(__dirname, 'fixture')

test('makeDedicatedLockfile()', async () => {
  const tmp = tempDir()
  await ncp(fixture, tmp)
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
