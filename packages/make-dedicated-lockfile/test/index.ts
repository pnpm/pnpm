import fs from 'node:fs'
import path from 'node:path'
import execa from 'execa'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { fixtures } from '@pnpm/test-fixtures'
import { makeDedicatedLockfile } from '../lib'

const f = fixtures(__dirname)
const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

test('makeDedicatedLockfile()', async () => {
  const tmp = f.prepare('fixture')
  fs.writeFileSync('.npmrc', 'store-dir=store\ncache-dir=cache', 'utf8')
  await execa('node', [
    pnpmBin,
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
