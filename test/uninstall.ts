import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import readPkg = require('read-pkg')
import {
  prepare,
  testDefaults,
} from './utils'
import {
  installPkgs,
} from 'supi'
import thenify = require('thenify')
import pnpmCli = require('../src/bin/pnpm')

test('uninstall package and remove from appropriate property', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['is-positive@3.1.0'], testDefaults({ saveOptional: true }))

  // testing the CLI directly as there was an issue where `npm.config` started to set save = true by default
  // npm@5 introduced --save-prod that bahaves the way --save worked in pre 5 versions
  await pnpmCli(['uninstall', 'is-positive'])

  await project.storeHasNot('is-positive', '3.1.0')

  await project.hasNot('is-positive')

  const pkgJson = await readPkg()
  t.equal(pkgJson.optionalDependencies, undefined, 'is-negative has been removed from optionalDependencies')
})
