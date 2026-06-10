import path from 'path'
import { preparePackage } from '@pnpm/prepare-package'
import { tempDir } from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { fixtures } from '@pnpm/test-fixtures'

const f = fixtures(__dirname)
const pkgResolutionId = 'https://codeload.example.com/org/repo/tar.gz/0000000000000000000000000000000000000000'
const allowBuild = () => true
const allowRegistryArtifactsOnly = (depPath: string) => !depPath.includes('://')

test('prepare package runs the prepublish script', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  f.copy('has-prepublish-script', tmp)
  await preparePackage({ allowBuild, pkgResolutionId, rawConfig: {} }, tmp, '')
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})

test('prepare package gates the build on the artifact depPath', async () => {
  const tmp = tempDir()
  f.copy('has-prepublish-script', tmp)

  await expect(preparePackage({
    allowBuild: allowRegistryArtifactsOnly,
    pkgResolutionId,
    rawConfig: {},
  }, tmp, '')).rejects.toThrow('needs to execute build scripts but is not in the "onlyBuiltDependencies" allowlist')
})

test('prepare package does not run the prepublish script if the main file is present', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  f.copy('has-prepublish-script-and-main-file', tmp)
  await preparePackage({ allowBuild, pkgResolutionId, rawConfig: {} }, tmp, '')
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})

test('prepare package runs the prepublish script in the sub folder if pkgDir is present', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  f.copy('has-prepublish-script-in-workspace', tmp)
  await preparePackage({ allowBuild, pkgResolutionId, rawConfig: {} }, tmp, 'packages/foo')
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})
