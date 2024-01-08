import path from 'path'
import { preparePackage } from '@pnpm/prepare-package'
import { tempDir } from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { fixtures } from '@pnpm/test-fixtures'

const f = fixtures(__dirname)

test('prepare package runs the prepublish script', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  f.copy('has-prepublish-script', tmp)
  await preparePackage({ rawConfig: {} }, tmp)
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})

test('prepare package does not run the prepublish script if the main file is present', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  f.copy('has-prepublish-script-and-main-file', tmp)
  await preparePackage({ rawConfig: {} }, tmp)
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})

test('prepare package runs the prepublish script when installing from monorepo and workspaces field is present', async () => {
  const tmp = tempDir()
  f.copy('has-workspaces-in-manifest', tmp)
  await expect(preparePackage({ rawConfig: {} }, tmp, true)).resolves.toBeTruthy()
})

test('prepare package runs the prepublish script when installing from monorepo and pnpm-workspace.yaml exists', async () => {
  const tmp = tempDir()
  f.copy('has-workspace-yaml', tmp)
  await preparePackage({ rawConfig: {} }, tmp)
  await expect(preparePackage({ rawConfig: {} }, tmp, true)).resolves.toBeTruthy()
})
