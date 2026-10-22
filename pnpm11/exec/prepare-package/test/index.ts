import fs from 'node:fs/promises'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackage } from '@pnpm/exec.prepare-package'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { createTestIpcServer } from '@pnpm/test-ipc-server'

const testFixtures = fixtures(import.meta.dirname)
const pkgResolutionId = 'https://codeload.example.com/org/repo/tar.gz/0000000000000000000000000000000000000000'
const allowBuild = () => true
const allowRegistryArtifactsOnly = (depPath: string) => depPath.includes('://') ? undefined : true

test('prepare package runs the prepublish script', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  testFixtures.copy('has-prepublish-script', tmp)
  await preparePackage({ allowBuild, pkgResolutionId }, tmp, '')
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})

test('prepare package gates the build on the artifact depPath', async () => {
  const tmp = tempDir()
  testFixtures.copy('has-prepublish-script', tmp)

  await expect(preparePackage({
    allowBuild: allowRegistryArtifactsOnly,
    pkgResolutionId,
  }, tmp, '')).rejects.toThrow('needs to execute build scripts but is not in the "allowBuilds" allowlist')
})

test('prepare package does not run the prepublish script if the main file is present', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  testFixtures.copy('has-prepublish-script-and-main-file', tmp)
  await preparePackage({ allowBuild, pkgResolutionId }, tmp, '')
  expect(server.getLines()).toStrictEqual([
    'prepublish',
  ])
})

test('prepare package runs the prepublish script in the sub folder if pkgDir is present', async () => {
  const tmp = tempDir()
  await using server = await createTestIpcServer(path.join(tmp, 'test.sock'))
  testFixtures.copy('has-prepublish-script-in-workspace', tmp)
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
      prepublish: 'node -e "console.log(process.env.pnpm_config_strict_dep_builds, process.env.PNPM_CONFIG_STRICT_DEP_BUILDS)" | test-ipc-server-client ./test.sock',
    },
  }))
  await preparePackage({ allowBuild, pkgResolutionId }, tmp, '')
  expect(server.getLines()).toStrictEqual([
    'false false',
  ])
})

test('prepare package installs a workspace that has no lockfile with pnpm', async () => {
  const tmp = tempDir()
  await fs.writeFile(path.join(tmp, 'pnpm-workspace.yaml'), 'packages: []\n')
  await fs.writeFile(path.join(tmp, 'package.json'), JSON.stringify({
    name: 'workspace-without-lockfile',
    version: '1.0.0',
    scripts: { prepare: 'node -e ""' },
  }))
  await preparePackage({ allowBuild, pkgResolutionId }, tmp, '')
  const files = await fs.readdir(tmp)
  expect(files).toContain('pnpm-lock.yaml')
  expect(files).not.toContain('package-lock.json')
})

const symlinkType = process.platform === 'win32' ? 'junction' : 'dir'

test('prepare package rejects intermediate symlink traversal pointing outside repository root', async () => {
  const tmp = tempDir()
  const outside = tempDir()
  const outsideSub = path.join(outside, 'sub')
  await fs.mkdir(outsideSub, { recursive: true })
  await fs.writeFile(path.join(outsideSub, 'package.json'), JSON.stringify({ name: 'outside-sub', version: '1.0.0' }))

  await fs.symlink(outside, path.join(tmp, 'external_link'), symlinkType)

  await expect(preparePackage({ allowBuild, pkgResolutionId }, tmp, 'external_link/sub')).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_PATH',
  })
})

test('prepare package rejects symlink directly pointing outside repository root', async () => {
  const tmp = tempDir()
  const outside = tempDir()
  await fs.writeFile(path.join(outside, 'package.json'), JSON.stringify({ name: 'outside', version: '1.0.0' }))

  await fs.symlink(outside, path.join(tmp, 'external_dir'), symlinkType)

  await expect(preparePackage({ allowBuild, pkgResolutionId }, tmp, 'external_dir')).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_PATH',
  })
})

test.each(['../outside', '..\\outside'])('prepare package rejects directory traversal with parent escape (%s)', async (subPath) => {
  const tmp = tempDir()
  await expect(preparePackage({ allowBuild, pkgResolutionId }, tmp, subPath)).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_PATH',
  })
})

test('prepare package accepts internal symlink pointing within repository root', async () => {
  const tmp = tempDir()
  const internalSub = path.join(tmp, 'packages', 'foo')
  await fs.mkdir(internalSub, { recursive: true })
  await fs.writeFile(path.join(internalSub, 'package.json'), JSON.stringify({ name: 'internal-sub', version: '1.0.0' }))

  await fs.symlink(path.join(tmp, 'packages', 'foo'), path.join(tmp, 'link_to_foo'), symlinkType)

  const result = await preparePackage({ allowBuild, pkgResolutionId }, tmp, 'link_to_foo')
  expect(result.pkgDir).toBe(path.join(tmp, 'link_to_foo'))
})

test('prepare package accepts a checkout root reached through a symlink', async () => {
  const tmp = tempDir()
  const checkout = path.join(tmp, 'checkout')
  const linkedCheckout = path.join(tmp, 'linked-checkout')
  await fs.mkdir(path.join(checkout, 'package'), { recursive: true })
  await fs.writeFile(path.join(checkout, 'package', 'package.json'), '{"name":"internal"}')
  await fs.symlink(checkout, linkedCheckout, 'junction')

  await expect(preparePackage({ pkgResolutionId }, linkedCheckout, 'package'))
    .resolves.toMatchObject({ pkgDir: path.join(linkedCheckout, 'package'), shouldBeBuilt: false })
})
