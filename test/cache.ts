import {installPkgs, install} from '../src'
import {prepare, addDistTag, testDefaults} from './utils'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import exists = require('path-exists')
import path = require('path')

const test = promisifyTape(tape)

test('should fail to update when requests are cached', async function (t) {
  const project = prepare(t)

  const metaCache = new Map()

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true, metaCache}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await install(testDefaults({depth: 1, metaCache, update: true}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('should not cache when cache is not used', async function (t) {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await install(testDefaults({depth: 1, update: true}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})
