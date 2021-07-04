import path from 'path'
import assertStore from '@pnpm/assert-store'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import {
  addDependenciesToPackage,
  install,
} from 'supi'
import exists from 'path-exists'
import sinon from 'sinon'
import { addDistTag, testDefaults } from '../utils'

test('install with lockfileOnly = true', async () => {
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  const project = prepareEmpty()

  const opts = await testDefaults({ lockfileOnly: true, pinnedVersion: 'patch' as const })
  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep@100.0.0'], opts)
  const { cafsHas } = assertStore(opts.storeDir)

  await cafsHas('pkg-with-1-dep', '100.0.0')
  expect(await exists(path.join(opts.cacheDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep.json`))).toBeTruthy()
  await cafsHas('dep-of-pkg-with-1-dep', '100.1.0')
  expect(await exists(path.join(opts.cacheDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/dep-of-pkg-with-1-dep.json`))).toBeTruthy()
  await project.hasNot('pkg-with-1-dep')

  expect(manifest.dependencies!['pkg-with-1-dep']).toBeTruthy()

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['pkg-with-1-dep']).toBeTruthy()
  expect(lockfile.packages['/pkg-with-1-dep/100.0.0']).toBeTruthy()
  expect(lockfile.specifiers['pkg-with-1-dep']).toBeTruthy()

  const currentLockfile = await project.readCurrentLockfile()
  expect(currentLockfile).toBeFalsy()

  console.log(`doing repeat install when ${WANTED_LOCKFILE} is available already`)
  await install(manifest, opts)

  await cafsHas('pkg-with-1-dep', '100.0.0')
  expect(await exists(path.join(opts.cacheDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep.json`))).toBeTruthy()
  await cafsHas('dep-of-pkg-with-1-dep', '100.1.0')
  expect(await exists(path.join(opts.cacheDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/dep-of-pkg-with-1-dep.json`))).toBeTruthy()
  await project.hasNot('pkg-with-1-dep')

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
  expect(lockfile.packages['/rimraf/2.5.1']).toBeTruthy()
  expect(lockfile.specifiers.rimraf).toBeTruthy()

  const currentLockfile = await project.readCurrentLockfile()
  expect(currentLockfile.packages['/rimraf/2.5.1']).toBeFalsy()
})
