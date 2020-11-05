import { WANTED_LOCKFILE } from '@pnpm/constants'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { lockfileMergeDriver } from '@pnpm/plugin-commands-merge-driver'
import fs = require('fs')
import path = require('path')
import tempy = require('tempy')

jest.setTimeout(20000)

test('merging dependencies that are resolving peers', async () => {
  const fixtureDir = path.join(__dirname, 'fixtures/peers')
  const outputDir = tempy.directory()
  process.chdir(outputDir)
  fs.copyFileSync(
    path.join(fixtureDir, 'ours/package.json'),
    path.join(outputDir, 'package.json')
  )
  await lockfileMergeDriver.handler({}, [
    path.join(fixtureDir, 'ours/pnpm-lock.yaml'),
    path.join(fixtureDir, 'base/pnpm-lock.yaml'),
    path.join(fixtureDir, 'theirs/pnpm-lock.yaml'),
    path.join(outputDir, WANTED_LOCKFILE),
  ])
  expect(
    await readWantedLockfile(outputDir, { ignoreIncompatible: false })
  ).toMatchSnapshot()
})
