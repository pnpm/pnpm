import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackage } from '@pnpm/exec.prepare-package'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { createTestIpcServer } from '@pnpm/test-ipc-server'

const f = fixtures(import.meta.dirname)
const allowBuild = () => true

test('prepare package runs the prepublish script', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  f.copy('has-prepublish-script', tmp)
  await preparePackage({ allowBuild }, tmp, '')
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})

test('prepare package does not run the prepublish script if the main file is present', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  f.copy('has-prepublish-script-and-main-file', tmp)
  await preparePackage({ allowBuild }, tmp, '')
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})

test('prepare package runs the prepublish script in the sub folder if pkgDir is present', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  f.copy('has-prepublish-script-in-workspace', tmp)
  await preparePackage({ allowBuild }, tmp, 'packages/foo')
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})
