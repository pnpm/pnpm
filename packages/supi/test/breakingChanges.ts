import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import isCI = require('is-ci')
import makeDir = require('make-dir')
import fs = require('mz/fs')
import path = require('path')
import rimraf = require('rimraf-then')
import { addDependenciesToPackage, install } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('fail on non-compatible node_modules', async (t: tape.Test) => {
  prepareEmpty(t)
  const opts = await testDefaults()

  await saveModulesYaml('0.50.0', opts.store)

  try {
    await addDependenciesToPackage({}, ['is-negative'], opts)
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.code, 'ERR_PNPM_MODULES_BREAKING_CHANGE', 'modules breaking change error is thrown')
  }
})

test("don't fail on non-compatible node_modules when forced", async (t: tape.Test) => {
  prepareEmpty(t)
  const opts = await testDefaults({ force: true })

  await saveModulesYaml('0.50.0', opts.store)

  await install({}, opts)

  t.pass('install did not fail')
})

test("don't fail on non-compatible node_modules when forced in a workspace", async (t: tape.Test) => {
  preparePackages(t, [
    {
      location: 'pkg',
      package: {},
    },
  ])
  const opts = await testDefaults({ force: true })

  process.chdir('pkg')
  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], await testDefaults({ lockfileDirectory: path.resolve('..') }))
  await rimraf('node_modules')

  process.chdir('..')

  await fs.writeFile('node_modules/.modules.yaml', `packageManager: pnpm@${3}\nstore: ${opts.store}\nindependentLeaves: false\nlayoutVersion: 1`)

  await install(manifest, { ...opts, prefix: path.resolve('pkg'), lockfileDirectory: process.cwd() })

  t.pass('install did not fail')
})

test('do not fail on non-compatible node_modules when forced with a named installation', async (t: tape.Test) => {
  prepareEmpty(t)
  const opts = await testDefaults({ force: true })

  await saveModulesYaml('0.50.0', opts.store)

  await addDependenciesToPackage({}, ['is-negative'], opts)
})

test("don't fail on non-compatible store when forced", async (t: tape.Test) => {
  prepareEmpty(t)
  const opts = await testDefaults({ force: true })

  await saveModulesYaml('0.32.0', opts.store)

  await install({}, opts)

  t.pass('install did not fail')
})

test('do not fail on non-compatible store when forced during named installation', async (t: tape.Test) => {
  prepareEmpty(t)
  const opts = await testDefaults({ force: true })

  await saveModulesYaml('0.32.0', opts.store)

  await addDependenciesToPackage({}, ['is-negative'], opts)
})

async function saveModulesYaml (pnpmVersion: string, storePath: string) {
  await makeDir('node_modules')
  await fs.writeFile('node_modules/.modules.yaml', `packageManager: pnpm@${pnpmVersion}\nstore: ${storePath}\nindependentLeaves: false`)
}

test(`fail on non-compatible ${WANTED_LOCKFILE}`, async (t: tape.Test) => {
  if (isCI) {
    t.skip('this test will always fail on CI servers')
    return
  }

  prepareEmpty(t)
  await fs.writeFile(WANTED_LOCKFILE, '')

  try {
    await addDependenciesToPackage({}, ['is-negative'], await testDefaults())
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.code, 'ERR_PNPM_LOCKFILE_BREAKING_CHANGE', 'lockfile breaking change error is thrown')
  }
})

test(`don't fail on non-compatible ${WANTED_LOCKFILE} when forced`, async (t: tape.Test) => {
  prepareEmpty(t)
  await fs.writeFile(WANTED_LOCKFILE, '')

  await addDependenciesToPackage({}, ['is-negative'], await testDefaults({ force: true }))

  t.pass('install did not fail')
})
