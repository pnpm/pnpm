import {installPkgs, install} from '../src'
import {add as addDistTag} from './support/distTags'
import testDefaults from './support/testDefaults'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import exists = require('exists-file')
import path = require('path')
import prepare from './support/prepare'

const test = promisifyTape(tape)

test('should fail to update when requests are cached', async function (t) {
  const project = prepare(t)

  const latest = 'stable'
  const metaCache = new Map()

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', latest)

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true, tag: latest, metaCache}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', latest)

  await install(testDefaults({depth: 1, tag: latest, metaCache}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('should not cache when cache is not used', async function (t) {
  const project = prepare(t)

  const latest = 'stable'

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', latest)

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true, tag: latest}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', latest)

  await install(testDefaults({depth: 1, tag: latest}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})
