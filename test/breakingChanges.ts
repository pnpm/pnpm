import tape = require('tape')
import promisifyTape from 'tape-promise'
import fs = require('mz/fs')
import mkdirp = require('mkdirp')
import path = require('path')
import {prepare, testDefaults} from './utils'
import {installPkgs} from '../src'

const test = promisifyTape(tape)

test('fail on non-compatible node_modules', async t => {
  const project = prepare(t)
  const opts = testDefaults()

  await saveModulesYaml('0.50.0', path.join(opts.storePath, '1'))

  try {
    await installPkgs(['is-negative'], opts)
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.code, 'MODULES_BREAKING_CHANGE', 'modules breaking change error is thrown')
  }
})

test("don't fail on non-compatible node_modules when forced", async t => {
  const project = prepare(t)
  const opts = testDefaults({force: true})

  await saveModulesYaml('0.50.0', path.join(opts.storePath, '1'))

  await installPkgs(['is-negative'], opts)

  t.pass('install did not fail')
})

test('fail on non-compatible store', async t => {
  const project = prepare(t)
  const opts = testDefaults()

  await saveModulesYaml('0.32.0', path.join(opts.storePath, '1'))

  try {
    await installPkgs(['is-negative'], opts)
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.code, 'STORE_BREAKING_CHANGE', 'store breaking change error is thrown')
  }
})

test("don't fail on non-compatible store when forced", async t => {
  const project = prepare(t)
  const opts = testDefaults({force: true})

  await saveModulesYaml('0.32.0', path.join(opts.storePath, '1'))

  await installPkgs(['is-negative'], opts)

  t.pass('install did not fail')
})

async function saveModulesYaml (pnpmVersion: string, storePath: string) {
  mkdirp.sync('node_modules')
  await fs.writeFile('node_modules/.modules.yaml', `packageManager: pnpm@${pnpmVersion}\nstorePath: ${storePath}`)
}
