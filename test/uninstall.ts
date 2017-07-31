import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import readPkg = require('read-pkg')
import {
  prepare,
  testDefaults,
  execPnpm,
} from './utils'
import {
  installPkgs,
} from 'supi'
import thenify = require('thenify')
import pnpmCli = require('../src/bin/pnpm')
import path = require('path')
import isWindows = require('is-windows')
import exists = require('path-exists')

test('uninstall package and remove from appropriate property', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['is-positive@3.1.0'], testDefaults({ saveOptional: true }))

  // testing the CLI directly as there was an issue where `npm.config` started to set save = true by default
  // npm@5 introduced --save-prod that bahaves the way --save worked in pre 5 versions
  await pnpmCli(['uninstall', 'is-positive'])

  await project.storeHas('is-positive', '3.1.0')

  await pnpmCli(['store', 'prune'])

  await project.storeHasNot('is-positive', '3.1.0')

  await project.hasNot('is-positive')

  const pkgJson = await readPkg()
  t.equal(pkgJson.optionalDependencies, undefined, 'is-negative has been removed from optionalDependencies')
})

test('uninstall global package with its bin files', async (t: tape.Test) => {
  prepare(t)
  process.chdir('..')

  const global = path.resolve('global')
  const globalBin = isWindows() ? global : path.join(global, 'bin')

  process.env.NPM_CONFIG_PREFIX = global

  await execPnpm('install', '-g', 'sh-hello-world@1.0.1')

  let stat = await exists(path.resolve(globalBin, 'sh-hello-world'))
  t.ok(stat, 'sh-hello-world is in .bin')

  await execPnpm('uninstall', '-g', 'sh-hello-world')

  stat = await exists(path.resolve(globalBin, 'sh-hello-world'))
  t.notOk(stat, 'sh-hello-world is removed from .bin')
})
