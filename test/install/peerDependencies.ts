import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import exists = require('path-exists')
import {installPkgs} from '../../src'
import {
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)
const NM = 'node_modules'

test("don't fail when peer dependency is fetched from GitHub", t => {
  const project = prepare(t)
  return installPkgs(['test-pnpm-peer-deps'], testDefaults())
})

test('peer dependency is linked', async t => {
  const project = prepare(t)
  await installPkgs(['ajv@4.10.4', 'ajv-keywords@1.5.0'], testDefaults())

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', 'ajv@4.10.4', NM, 'ajv')), 'peer dependency is linked')
})

test('peer dependency is linked on subsequent install', async t => {
  const project = prepare(t)

  await installPkgs(['ajv@4.10.4'], testDefaults())

  await installPkgs(['ajv-keywords@1.5.0'], testDefaults())

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', 'ajv@4.10.4', NM, 'ajv')), 'peer dependency is linked')
})

test('peer dependencies are linked', async t => {
  const project = prepare(t)
  await installPkgs(['abc-parent-with-ab', 'abc-grand-parent-with-c', 'peer-c@2.0.0'], testDefaults())

  const pkgVariationsDir = path.join(NM, '.localhost+4873', 'abc', '1.0.0')
  t.ok(await exists(path.join(pkgVariationsDir, NM, 'dep-of-pkg-with-1-dep')))

  const pkgVariation1 = path.join(pkgVariationsDir, 'peer-a@1.0.0+peer-b@1.0.0+peer-c@1.0.0', NM)
  t.ok(await exists(path.join(pkgVariation1, 'abc')))
  t.ok(await exists(path.join(pkgVariation1, 'peer-a')))
  t.ok(await exists(path.join(pkgVariation1, 'peer-b')))
  t.ok(await exists(path.join(pkgVariation1, 'peer-c')))

  const pkgVariation2 = path.join(pkgVariationsDir, 'peer-a@1.0.0+peer-b@1.0.0+peer-c@2.0.0', NM)
  t.ok(await exists(path.join(pkgVariation2, 'abc')))
  t.ok(await exists(path.join(pkgVariation2, 'peer-a')))
  t.ok(await exists(path.join(pkgVariation2, 'peer-b')))
  t.ok(await exists(path.join(pkgVariation2, 'peer-c')))
})

test('scoped peer dependency is linked', async t => {
  const project = prepare(t)
  await installPkgs(['@having/scoped-peer', '@scoped/peer'], testDefaults())

  const pkgVariation = path.join(NM, '.localhost+4873', '@having', 'scoped-peer', '1.0.0', '@scoped!peer@1.0.0', NM)
  t.ok(await exists(path.join(pkgVariation, '@having', 'scoped-peer')))
  t.ok(await exists(path.join(pkgVariation, '@scoped', 'peer')))
})

test('peer bins are linked', async t => {
  const project = prepare(t)

  await installPkgs(['pkg-with-peer-having-bin', 'peer-with-bin'], testDefaults())

  const pkgVariation = path.join('.localhost+4873', 'pkg-with-peer-having-bin', '1.0.0', 'peer-with-bin@1.0.0', NM)

  await project.isExecutable(path.join(pkgVariation, 'pkg-with-peer-having-bin', NM, '.bin', 'peer-with-bin'))

  await project.isExecutable(path.join(pkgVariation, 'pkg-with-peer-having-bin', NM, '.bin', 'hello-world-js-bin'))
})

test('run pre/postinstall scripts of each variations of packages with peer dependencies', async t => {
  const project = prepare(t)
  await installPkgs(['parent-of-pkg-with-events-and-peers', 'pkg-with-events-and-peers', 'peer-c@2.0.0'], testDefaults())

  const pkgVariation1 = path.join(NM, '.localhost+4873', 'pkg-with-events-and-peers', '1.0.0', 'peer-c@1.0.0', NM)
  t.ok(await exists(path.join(pkgVariation1, 'pkg-with-events-and-peers', 'generated-by-preinstall.js')))
  t.ok(await exists(path.join(pkgVariation1, 'pkg-with-events-and-peers', 'generated-by-postinstall.js')))

  const pkgVariation2 = path.join(NM, '.localhost+4873', 'pkg-with-events-and-peers', '1.0.0', 'peer-c@2.0.0', NM)
  t.ok(await exists(path.join(pkgVariation2, 'pkg-with-events-and-peers', 'generated-by-preinstall.js')))
  t.ok(await exists(path.join(pkgVariation2, 'pkg-with-events-and-peers', 'generated-by-postinstall.js')))
})

test('package that resolves its own peer dependency', async t => {
  const project = prepare(t)
  await installPkgs(['pkg-with-resolved-peer', 'peer-c@2.0.0'], testDefaults())

  t.ok(await exists(path.join(NM, '.localhost+4873', 'pkg-with-resolved-peer', '1.0.0', 'peer-c@1.0.0', NM, 'pkg-with-resolved-peer')))
})
