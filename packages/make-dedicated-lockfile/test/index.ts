import path from 'path'
import execa from 'execa'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { fixtures } from '@pnpm/test-fixtures'
import { makeDedicatedLockfile } from '../lib/index.js'

const f = fixtures(import.meta.dirname)
const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')

test('makeDedicatedLockfile()', async () => {
  const tmp = f.prepare('fixture')
  await execa('node', [
    pnpmBin,
    '--config.store-dir=store',
    '--config.cache-dir=cache',
    'install',
    '--no-frozen-lockfile',
    '--no-prefer-frozen-lockfile',
    '--force',
  ], { cwd: tmp })
  const projectDir = path.join(tmp, 'packages/is-negative')
  await makeDedicatedLockfile(tmp, projectDir)

  const lockfile = await readWantedLockfile(projectDir, { ignoreIncompatible: false })
  // The next assertion started failing from pnpm v10.6.3
  // expect(Object.keys(lockfile?.importers ?? {})).toStrictEqual(['.', 'example'])
  expect(Object.keys(lockfile?.packages ?? {}).sort()).toStrictEqual([
    'is-positive@1.0.0',
    'lodash@1.0.0',
    'ramda@0.26.0',
    'request@2.0.0',
  ])
})
