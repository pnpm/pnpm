import {cleanCache, installPkgs, install} from '../src'
import {add as addDistTag} from './support/distTags'
import testDefaults from './support/testDefaults'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import exists = require('exists-file')
import path = require('path')
import prepare from './support/prepare'

const test = promisifyTape(tape)

test('cache clean removes cache', async function (t) {
  prepare(t)

  const opts = testDefaults({global: true})

  await installPkgs(['is-positive'], opts)

  const cache = path.join(opts.globalPath, 'cache')

  t.ok(await exists(cache), 'cache is created')

  await cleanCache(opts.globalPath)

  t.ok(!await exists(cache), 'cache is removed')
})

test('should fail to update when requests are cached', async function (t) {
  const project = prepare(t)

  const latest = 'stable'
  const cacheTTL = 60 * 60

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', latest)

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true, tag: latest, cacheTTL}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', latest)

  await install(testDefaults({depth: 1, tag: latest, cacheTTL}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('should skip cahe even if it exists when cacheTTL = 0', async function (t) {
  const project = prepare(t)

  const latest = 'stable'

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', latest)

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true, tag: latest, cacheTTL: 60 * 60}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', latest)

  await install(testDefaults({depth: 1, tag: latest, cacheTTL: 0}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})
