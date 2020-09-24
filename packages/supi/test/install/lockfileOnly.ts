import assertStore from '@pnpm/assert-store'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import {
  addDependenciesToPackage,
  install,
} from 'supi'
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'
import path = require('path')
import exists = require('path-exists')
import sinon = require('sinon')
import tape = require('tape')

const test = promisifyTape(tape)

test('install with lockfileOnly = true', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const opts = await testDefaults({ lockfileOnly: true, pinnedVersion: 'patch' as const })
  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep@100.0.0'], opts)
  const { cafsHas } = assertStore(t, opts.storeDir)

  await cafsHas('pkg-with-1-dep', '100.0.0')
  t.ok(await exists(path.join(opts.storeDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep.json`)))
  await cafsHas('dep-of-pkg-with-1-dep', '100.1.0')
  t.ok(await exists(path.join(opts.storeDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/dep-of-pkg-with-1-dep.json`)))
  await project.hasNot('pkg-with-1-dep')

  t.ok(manifest.dependencies!['pkg-with-1-dep'], 'the new dependency added to package.json')

  const lockfile = await project.readLockfile()
  t.ok(lockfile.dependencies['pkg-with-1-dep'])
  t.ok(lockfile.packages['/pkg-with-1-dep/100.0.0'])
  t.ok(lockfile.specifiers['pkg-with-1-dep'])

  const currentLockfile = await project.readCurrentLockfile()
  t.notOk(currentLockfile, 'current lockfile not created')

  t.comment(`doing repeat install when ${WANTED_LOCKFILE} is available already`)
  await install(manifest, opts)

  await cafsHas('pkg-with-1-dep', '100.0.0')
  t.ok(await exists(path.join(opts.storeDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep.json`)))
  await cafsHas('dep-of-pkg-with-1-dep', '100.1.0')
  t.ok(await exists(path.join(opts.storeDir, `metadata/localhost+${REGISTRY_MOCK_PORT}/dep-of-pkg-with-1-dep.json`)))
  await project.hasNot('pkg-with-1-dep')

  t.notOk(await project.readCurrentLockfile(), 'current lockfile not created')
})

test('warn when installing with lockfileOnly = true and node_modules exists', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults())
  await addDependenciesToPackage(manifest, ['rimraf@2.5.1'], await testDefaults({
    lockfileOnly: true,
    reporter,
  }))

  t.ok(reporter.calledWithMatch({
    level: 'warn',
    message: '`node_modules` is present. Lockfile only installation will make it out-of-date',
    name: 'pnpm',
  }), 'log warning')

  await project.storeHas('rimraf', '2.5.1')
  await project.hasNot('rimraf')

  t.ok(manifest.dependencies!.rimraf, 'the new dependency added to package.json')

  const lockfile = await project.readLockfile()
  t.ok(lockfile.dependencies.rimraf)
  t.ok(lockfile.packages['/rimraf/2.5.1'])
  t.ok(lockfile.specifiers.rimraf)

  const currentLockfile = await project.readCurrentLockfile()
  t.notOk(currentLockfile.packages['/rimraf/2.5.1'], 'current lockfile not changed')
})
