import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import {installPkgs, prune, prunePkgs} from '../src'
import {prepare, testDefaults} from './utils'
import exists = require('exists-file')
import existsSymlink = require('exists-link')

test('prune removes extraneous packages', async function (t) {
  const project = prepare(t)

  await installPkgs(['is-negative@2.1.0'], testDefaults({save: true}))
  await installPkgs(['applyq@0.2.1'], testDefaults({saveDev: true}))
  await installPkgs(['fnumber@0.1.0'], testDefaults({saveOptional: true}))
  await installPkgs(['is-positive@2.0.0', '@zkochan/logger@0.1.0'], testDefaults())
  await prune(testDefaults())

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

test('prune removes only the specified extraneous packages', async function (t) {
  const project = prepare(t)

  await installPkgs(['is-positive@2.0.0', 'is-negative@2.1.0'], testDefaults())
  await prunePkgs(['is-positive'], testDefaults())

  await project.storeHasNot('is-positive', '2.0.0')
  await project.hasNot('is-positive')

  await project.storeHas('is-negative', '2.1.0')
  await project.has('is-negative')
})

test('prune throws error when trying to removes not an extraneous package', async function (t) {
  prepare(t)

  await installPkgs(['is-positive@2.0.0'], testDefaults({save: true}))

  try {
    await prunePkgs(['is-positive'], testDefaults())
    t.fail('prune had to fail')
  } catch (err) {
    t.equal(err['code'], 'PRUNE_NOT_EXTR', 'cannot prune non-extraneous package error thrown')
  }
})

test('prune removes dev dependencies in production', async function (t) {
  const project = prepare(t)

  await installPkgs(['is-positive@2.0.0'], testDefaults({saveDev: true}))
  await installPkgs(['is-negative@2.1.0'], testDefaults({save: true}))
  await installPkgs(['fnumber@0.1.0'], testDefaults({saveOptional: true}))
  await prune(testDefaults({production: true}))

  await project.storeHasNot('is-positive', '2.0.0')
  await project.hasNot('is-positive')

  await project.storeHas('is-negative', '2.1.0')
  await project.has('is-negative')

  await project.storeHas('fnumber', '0.1.0')
  await project.has('fnumber')
})
