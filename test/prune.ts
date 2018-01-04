import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import {installPkgs, prune} from 'supi'
import {prepare, testDefaults} from './utils'
import exists = require('path-exists')
import existsSymlink = require('exists-link')
import readPkg = require('read-pkg')
import writePkg = require('write-pkg')

test('prune removes extraneous packages', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-negative@2.1.0'], await testDefaults({save: true}))
  await installPkgs(['applyq@0.2.1'], await testDefaults({saveDev: true}))
  await installPkgs(['fnumber@0.1.0'], await testDefaults({saveOptional: true}))
  await installPkgs(['is-positive@2.0.0', '@zkochan/logger@0.1.0'], await testDefaults())

  const pkg = await readPkg()

  delete pkg.dependencies['is-positive']
  delete pkg.dependencies['@zkochan/logger']

  await writePkg(pkg)

  await prune(await testDefaults())

  await project.storeHasNot('is-positive', '2.0.0')
  await project.hasNot('is-positive')

  await project.storeHasNot('@zkochan/logger', '0.1.0')
  await project.hasNot('@zkochan/logger')

  await project.storeHas('is-negative', '2.1.0')
  await project.has('is-negative')

  await project.storeHas('applyq', '0.2.1')
  await project.has('applyq')

  await project.storeHas('fnumber', '0.1.0')
  await project.has('fnumber')
})

test('prune removes dev dependencies in production', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-positive@2.0.0'], await testDefaults({saveDev: true}))
  await installPkgs(['is-negative@2.1.0'], await testDefaults({save: true}))
  await installPkgs(['fnumber@0.1.0'], await testDefaults({saveOptional: true}))
  await prune(await testDefaults({
    production: true,
    development: false,
  }))

  await project.storeHasNot('is-positive', '2.0.0')
  await project.hasNot('is-positive')

  await project.storeHas('is-negative', '2.1.0')
  await project.has('is-negative')

  await project.storeHas('fnumber', '0.1.0')
  await project.has('fnumber')
})
