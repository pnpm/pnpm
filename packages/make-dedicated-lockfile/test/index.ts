import { readWantedLockfile } from '@pnpm/lockfile-file'
import ncpCB = require('ncp')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')
import { promisify } from 'util'
import makeDedicatedLockfile from '../lib'

const ncp = promisify(ncpCB)

const fixture = path.join(__dirname, 'fixture')

test('makeDedicatedLockfile()', async (t) => {
  const tmp = tempy.directory()
  await ncp(fixture, tmp)
  const projectDir = path.join(tmp, 'packages/is-negative')
  await makeDedicatedLockfile(tmp, projectDir)

  const lockfile = await readWantedLockfile(projectDir, { ignoreIncompatible: false })
  t.deepEqual(Object.keys(lockfile.importers), ['.', 'example'])
  t.deepEqual(Object.keys(lockfile.packages), [
    '/is-positive/1.0.0',
    '/lodash/1.0.0',
    '/ramda/0.26.0',
  ])
  t.end()
})
