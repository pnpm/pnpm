import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import isSubdir = require('is-subdir')
import path = require('path')
import resolveLinkTarget = require('resolve-link-target')
import { addDependenciesToPackage, install } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)

test('install with --independent-leaves', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep@100.0.0'], await testDefaults({ independentLeaves: true }))

  await project.has('pkg-with-1-dep')

  await install(manifest, await testDefaults({ independentLeaves: true, preferFrozenLockfile: false }))

  t.ok(isSubdir(path.resolve('node_modules'), await resolveLinkTarget(path.resolve('node_modules/pkg-with-1-dep'))), 'non-independent package is not symlinked directly from store')
})

test('--independent-leaves throws exception when executed on node_modules installed w/o the option', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const opts = await testDefaults({ independentLeaves: false })
  const manifest = await addDependenciesToPackage({}, ['is-positive'], opts)

  try {
    await addDependenciesToPackage(manifest, ['is-negative'], {
      ...opts,
      forceIndependentLeaves: true,
      independentLeaves: true,
    })
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err['code'], 'ERR_PNPM_INDEPENDENT_LEAVES_NOT_WANTED') // tslint:disable-line:no-string-literal
    t.ok(err.message.indexOf('This modules directory was created without the --independent-leaves option.') === 0)
  }

  // Install doesn't fail if independentLeaves is not forced
  await addDependenciesToPackage(manifest, ['is-negative'], {
    ...opts,
    forceIndependentLeaves: false,
    independentLeaves: true,
    lock: false,
  })

  await project.has('is-negative')
})

test('--no-independent-leaves throws exception when executed on node_modules installed with --independent-leaves', async (t: tape.Test) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ independentLeaves: true }))

  try {
    await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({
      forceIndependentLeaves: true,
      independentLeaves: false,
    }))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err['code'], 'ERR_PNPM_INDEPENDENT_LEAVES_WANTED') // tslint:disable-line:no-string-literal
    t.ok(err.message.indexOf('This modules directory was created using the --independent-leaves option.') === 0)
  }
})

// Covers https://github.com/pnpm/pnpm/issues/1547
test('installing with independent-leaves and hoistPattern', async (t) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['pkg-with-1-dep@100.0.0'], await testDefaults({
    hoistPattern: '*',
    independentLeaves: true,
  }))

  await project.has('pkg-with-1-dep')
  await project.has('.pnpm/node_modules/dep-of-pkg-with-1-dep')

  // wrappy is linked directly from the store
  await project.hasNot(`.pnpm/dep-of-pkg-with-1-dep@100.0.0`)
  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await project.has(`.pnpm/pkg-with-1-dep@100.0.0`)
})
