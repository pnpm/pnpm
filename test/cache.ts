import path = require('path')
import exists = require('path-exists')
import {install, installPkgs} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  prepare,
  testDefaults,
} from './utils'

const test = promisifyTape(tape)

test('should fail to update when requests are cached', async (t) => {
  const project = prepare(t)

  const metaCache = new Map()

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await installPkgs(['pkg-with-1-dep'], await testDefaults({save: true, metaCache}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await install(await testDefaults({depth: 1, metaCache, update: true}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('should not cache when cache is not used', async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await installPkgs(['pkg-with-1-dep'], await testDefaults({save: true}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await install(await testDefaults({depth: 1, update: true}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})
