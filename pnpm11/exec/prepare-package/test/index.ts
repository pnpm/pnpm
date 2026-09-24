import fs from 'node:fs/promises'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackage } from '@pnpm/exec.prepare-package'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { createTestIpcServer } from '@pnpm/test-ipc-server'

const f = fixtures(import.meta.dirname)
const pkgResolutionId = 'https://codeload.example.com/org/repo/tar.gz/0000000000000000000000000000000000000000'
const allowBuild = () => true
const allowRegistryArtifactsOnly = (depPath: string) => depPath.includes('://') ? undefined : true

test('prepare package runs the prepublish script', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  f.copy('has-prepublish-script', tmp)
  await preparePackage({ allowBuild, pkgResolutionId }, tmp, '')
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
  }, tmp, '')).rejects.toThrow('needs to execute build scripts but is not in the "allowBuilds" allowlist')
})

test('prepare package does not run the prepublish script if the main file is present', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  f.copy('has-prepublish-script-and-main-file', tmp)
  await preparePackage({ allowBuild, pkgResolutionId }, tmp, '')
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})

test('prepare package runs the prepublish script in the sub folder if pkgDir is present', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  f.copy('has-prepublish-script-in-workspace', tmp)
  await preparePackage({ allowBuild, pkgResolutionId }, tmp, 'packages/foo')
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})

test('explicitly denied preparation installs the source without running lifecycle scripts', async () => {
  const tmp = tempDir()
  await fs.writeFile(path.join(tmp, 'package.json'), JSON.stringify({
    name: 'denied-build',
    version: '1.0.0',
    scripts: { prepare: 'exit 1', preinstall: 'exit 1', postinstall: 'exit 1' },
  }))
  await fs.writeFile(path.join(tmp, 'index.js'), 'module.exports = 42')
  const result = await preparePackage({ allowBuild: () => false, pkgResolutionId }, tmp, '')
  expect(result).toEqual({ shouldBeBuilt: true, pkgDir: tmp, ignoredBuild: true })
  expect(await fs.readFile(path.join(tmp, 'index.js'), 'utf8')).toBe('module.exports = 42')
})

test('prepare package runs its scripts with strictDepBuilds off', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  await fs.writeFile(path.join(tmp, 'pnpm-lock.yaml'), '')
  await fs.writeFile(path.join(tmp, 'package.json'), JSON.stringify({
    name: 'records-strict-dep-builds',
    version: '1.0.0',
    scripts: {
      prepublish: 'node -e "console.log(process.env.pnpm_config_strict_dep_builds)" | test-ipc-server-client ./test.sock',
    },
  }))
  await preparePackage({ allowBuild, pkgResolutionId }, tmp, '')
  expect(server.getLines()).toStrictEqual([
    'false',
  ])
})
