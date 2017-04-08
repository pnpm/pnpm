import tape = require('tape')
import promisifyTape from 'tape-promise'
import rimraf = require('rimraf-then')
import {prepare, testDefaults, execPnpm} from './utils'
import {verify, installPkgs} from '../src'
import readPkg = require('read-pkg')
import writePkg = require('write-pkg')
import fs = require('mz/fs')

const test = promisifyTape(tape)

test('verify passes when everything is up to date', async function (t) {
  const project = prepare(t)

  await installPkgs(['is-negative@2.1.0'], testDefaults())

  const err = await verify(process.cwd())

  t.ok(!err, 'no errors')
})

test('verify fails when shrinkwrap.yaml is not up to date', async function (t) {
  const project = prepare(t)

  await installPkgs(['is-negative@2.1.0'], testDefaults())

  const pkg = await readPkg()

  pkg.dependencies['is-positive'] = '1.0.0'

  await writePkg(pkg)

  const err = await verify(process.cwd())

  t.equal(err!.code, 'OUTDATED_SHRINKWRAP_FILE', 'correct error code')
})

test('verify fails when node_modules is not up to date', async function (t) {
  const project = prepare(t)

  await installPkgs(['is-negative@2.1.0'], testDefaults())

  await fs.rename('node_modules', 'tmp')

  await installPkgs(['is-positive@3.1.0'], testDefaults())

  await rimraf('node_modules')

  await fs.rename('tmp', 'node_modules')

  const err = await verify(process.cwd())

  t.equal(err!.code, 'OUTDATED_NODE_MODULES', 'correct error code')
})

test('CLI fails with 0 exit code when exit-0 is passed', async function (t: tape.Test) {
  const project = prepare(t)

  await installPkgs(['is-negative@2.1.0'], testDefaults())

  const pkg = await readPkg()

  pkg.dependencies['is-positive'] = '1.0.0'

  await writePkg(pkg)

  try {
    await execPnpm('verify', '--exit-0')
    t.pass()
  } catch (err) {
    t.fail()
  }
})

test('CLI fails with exit code 1 when exit-0 is not passed', async function (t: tape.Test) {
  const project = prepare(t)

  await installPkgs(['is-negative@2.1.0'], testDefaults())

  const pkg = await readPkg()

  pkg.dependencies['is-positive'] = '1.0.0'

  await writePkg(pkg)

  try {
    await execPnpm('verify')
    t.fail()
  } catch (err) {
    t.pass()
  }
})
