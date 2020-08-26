import { execPnpm } from './utils'
import { fromDir as readPkgFromDir } from '@pnpm/read-package-json'
import prepare from '@pnpm/prepare'
import promisifyTape from 'tape-promise'
import fs = require('mz/fs')
import path = require('path')
import exists = require('path-exists')
import PATH = require('path-name')
import tape = require('tape')

const test = promisifyTape(tape)

test('uninstall package and remove from appropriate property', async function (t: tape.Test) {
  const project = prepare(t)
  await execPnpm(['install', '--save-optional', 'is-positive@3.1.0'])

  // testing the CLI directly as there was an issue where `npm.config` started to set save = true by default
  // npm@5 introduced --save-prod that bahaves the way --save worked in pre 5 versions
  await execPnpm(['uninstall', 'is-positive'])

  await project.storeHas('is-positive', '3.1.0')

  await execPnpm(['store', 'prune'])

  await project.storeHasNot('is-positive', '3.1.0')

  await project.hasNot('is-positive')

  const pkgJson = await readPkgFromDir(process.cwd())
  t.equal(pkgJson.optionalDependencies, undefined, 'is-negative has been removed from optionalDependencies')
})

test('uninstall global package with its bin files', async (t: tape.Test) => {
  prepare(t)
  process.chdir('..')

  const global = path.resolve('global')
  const globalBin = path.join(global, 'nodejs')
  await fs.mkdir(globalBin, { recursive: true })

  const env = {
    NPM_CONFIG_PREFIX: global,
    [PATH]: `${globalBin}${path.delimiter}${process.env[PATH] ?? ''}`,
  }
  if (process.env.APPDATA) env['APPDATA'] = global

  await execPnpm(['add', '-g', 'sh-hello-world@1.0.1'], { env })

  let stat = await exists(path.resolve(globalBin, 'sh-hello-world'))
  t.ok(stat, 'sh-hello-world is in .bin')

  await execPnpm(['uninstall', '-g', 'sh-hello-world'], { env })

  stat = await exists(path.resolve(globalBin, 'sh-hello-world'))
  t.notOk(stat, 'sh-hello-world is removed from .bin')
})
