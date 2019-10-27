import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import fs = require('mz/fs')
import path = require('path')
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  install,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('install with lockfileOnly = true', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const opts = await testDefaults({ lockfileOnly: true, pinnedVersion: 'patch' })
  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep@100.0.0'], opts)

  t.deepEqual(await fs.readdir(path.join(opts.storeDir, 'localhost+4873', 'pkg-with-1-dep')), ['100.0.0', 'index.json'])
  t.deepEqual(await fs.readdir(path.join(opts.storeDir, 'localhost+4873', 'dep-of-pkg-with-1-dep')), ['100.1.0', 'index.json'])
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

  t.deepEqual(await fs.readdir(path.join(opts.storeDir, 'localhost+4873', 'pkg-with-1-dep')), ['100.0.0', 'index.json'])
  t.deepEqual(await fs.readdir(path.join(opts.storeDir, 'localhost+4873', 'dep-of-pkg-with-1-dep')), ['100.1.0', 'index.json'])
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
