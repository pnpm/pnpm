import prepare from '@pnpm/prepare'
import isSubdir = require('is-subdir')
import path = require('path')
import resolveLinkTarget = require('resolve-link-target')
import { addDependenciesToPackage, install } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('install with --independent-leaves', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['rimraf@2.5.1'], await testDefaults({ independentLeaves: true }))

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')
  await project.isExecutable('.bin/rimraf')

  await install(await testDefaults({ independentLeaves: true, preferFrozenLockfile: false }))

  t.ok(isSubdir(path.resolve('node_modules'), await resolveLinkTarget(path.resolve('node_modules/rimraf'))), 'non-independent package is not symlinked directly from store')
})

test('--independent-leaves throws exception when executed on node_modules installed w/o the option', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['is-positive'], await testDefaults({ independentLeaves: false }))

  try {
    await addDependenciesToPackage(['is-negative'], await testDefaults({ independentLeaves: true }))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err['code'], 'ERR_PNPM_INDEPENDENT_LEAVES_NOT_WANTED') // tslint:disable-line:no-string-literal
    t.ok(err.message.indexOf('This "node_modules" folder was created without the --independent-leaves option.') === 0)
  }
})

test('--no-independent-leaves throws exception when executed on node_modules installed with --independent-leaves', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['is-positive'], await testDefaults({ independentLeaves: true }))

  try {
    await addDependenciesToPackage(['is-negative'], await testDefaults({ independentLeaves: false }))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err['code'], 'ERR_PNPM_INDEPENDENT_LEAVES_WANTED') // tslint:disable-line:no-string-literal
    t.ok(err.message.indexOf('This "node_modules" folder was created using the --independent-leaves option.') === 0)
  }
})

// Covers https://github.com/pnpm/pnpm/issues/1547
test('installing with independent-leaves and shamefully-flatten', async (t) => {
  const project = prepare(t)
  await addDependenciesToPackage(['rimraf@2.5.1'], await testDefaults({
    independentLeaves: true,
    shamefullyFlatten: true,
  }))

  await project.has('rimraf')
  await project.has('minimatch')

  // wrappy is linked directly from the store
  await project.hasNot('.localhost+4873/wrappy/1.0.2')
  await project.storeHas('wrappy', '1.0.2')

  await project.has('.localhost+4873/rimraf/2.5.1')
})
