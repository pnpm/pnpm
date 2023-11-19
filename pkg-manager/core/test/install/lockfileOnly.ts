import path from 'path'
import { promises as fs } from 'fs'
import { assertStore } from '@pnpm/assert-store'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT, addDistTag } from '@pnpm/registry-mock'
import {
  addDependenciesToPackage,
  install,
} from '@pnpm/core'
import exists from 'path-exists'
import sinon from 'sinon'
import { testDefaults } from '../utils'

test('install with lockfileOnly = true', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  const project = prepareEmpty()

  const opts = await testDefaults({ lockfileOnly: true, pinnedVersion: 'patch' as const })
  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'], opts)
  const { cafsHas } = assertStore(opts.storeDir)

  await cafsHas('@pnpm.e2e/pkg-with-1-dep', '100.0.0')
  expect(await exists(path.join(opts.cacheDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/pkg-with-1-dep.json`))).toBeTruthy()
  await cafsHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0')
  expect(await exists(path.join(opts.cacheDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/dep-of-pkg-with-1-dep.json`))).toBeTruthy()
  await project.hasNot('@pnpm.e2e/pkg-with-1-dep')

  expect(manifest.dependencies!['@pnpm.e2e/pkg-with-1-dep']).toBeTruthy()

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['@pnpm.e2e/pkg-with-1-dep']).toBeTruthy()
  expect(lockfile.packages['/@pnpm.e2e/pkg-with-1-dep@100.0.0']).toBeTruthy()

  const currentLockfile = await project.readCurrentLockfile()
  expect(currentLockfile).toBeFalsy()

  console.log(`doing repeat install when ${WANTED_LOCKFILE} is available already`)
  await install(manifest, opts)

  await cafsHas('@pnpm.e2e/pkg-with-1-dep', '100.0.0')
  expect(await exists(path.join(opts.cacheDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/pkg-with-1-dep.json`))).toBeTruthy()
  await cafsHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0')
  expect(await exists(path.join(opts.cacheDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/dep-of-pkg-with-1-dep.json`))).toBeTruthy()
  await project.hasNot('@pnpm.e2e/pkg-with-1-dep')

  expect(await project.readCurrentLockfile()).toBeFalsy()
})

test('warn when installing with lockfileOnly = true and node_modules exists', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults())
  await addDependenciesToPackage(manifest, ['rimraf@2.5.1'], await testDefaults({
    lockfileOnly: true,
    reporter,
  }))

  expect(reporter.calledWithMatch({
    level: 'warn',
    message: '`node_modules` is present. Lockfile only installation will make it out-of-date',
    name: 'pnpm',
  })).toBeTruthy()

  await project.storeHas('rimraf', '2.5.1')
  await project.hasNot('rimraf')

  expect(manifest.dependencies!.rimraf).toBeTruthy()

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies.rimraf).toBeTruthy()
  expect(lockfile.packages['/rimraf@2.5.1']).toBeTruthy()

  const currentLockfile = await project.readCurrentLockfile()
  expect(currentLockfile.packages['/rimraf@2.5.1']).toBeFalsy()
})

test('do not update the lockfile when lockfileOnly and frozenLockfile are both used', async () => {
  prepareEmpty()
  await addDependenciesToPackage({}, ['is-positive@1.0.0'], await testDefaults({
    lockfileOnly: true,
  }))
  await expect(install({
    dependencies: {
      'is-positive': '2.0.0',
    },
  }, await testDefaults({
    lockfileOnly: true,
    frozenLockfile: true,
  }))).rejects.toThrow(/is not up to date/)
})

test('a lockfile only update with the useExperimentalNpmjsFilesIndex flag resolves without packages being fetched', async () => {
  const project = prepareEmpty()
  const opts = await testDefaults({ useExperimentalNpmjsFilesIndex: true, lockfileOnly: true })
  await addDependenciesToPackage({}, ['nodecv@1.1.2'], opts)
  const lockfile = await project.readLockfile()
  expect(lockfile.packages!['/nodecv@1.1.2'].requiresBuild).toBeTruthy()
  expect(await fs.readdir(opts.storeDir)).toStrictEqual([])
})
