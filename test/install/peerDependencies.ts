import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import exists = require('path-exists')
import {installPkgs} from '../../src'
import {
  prepare,
  testDefaults,
} from '../utils'
import streamParser from '../../src/logging/streamParser'
import deepRequireCwd = require('deep-require-cwd')

const test = promisifyTape(tape)
const NM = 'node_modules'

test("don't fail when peer dependency is fetched from GitHub", t => {
  const project = prepare(t)
  return installPkgs(['test-pnpm-peer-deps'], testDefaults())
})

test('peer dependency is grouped with dependency when peer is resolved not from a top dependency', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['using-ajv'], testDefaults())

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', 'ajv@4.10.4', NM, 'ajv')), 'peer dependency is linked')
  t.equal(deepRequireCwd(['using-ajv', 'ajv-keywords', 'ajv', './package.json']).version, '4.10.4')
})

test('peer dependency is not grouped with dependent when the peer is a top dependency', async (t: tape.Test) => {
  const project = prepare(t)

  let log: string | null = null

  function reporter (logObj: Object) {
    if (logObj['message'] && logObj['message'].indexOf('requires a peer of ajv@>=4.10.0 but none was installed.') !== -1) {
      log = logObj['message']
    }
  }

  streamParser.on('data', reporter)

  await installPkgs(['ajv@4.10.4', 'ajv-keywords@1.5.0'], testDefaults())

  streamParser['removeListener']('data', reporter)

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', NM, 'ajv-keywords')), 'dependent is at the normal location')
  t.ok(!log, 'no warning is logged about unresolved peer dep')
})

test('warning is reported when cannot resolve peer dependency', async (t: tape.Test) => {
  const project = prepare(t)
  let log: string | null = null

  function reporter (logObj: Object) {
    if (logObj['message'] && logObj['message'].indexOf('requires a peer of ajv@>=4.10.0 but none was installed.') !== -1) {
      log = logObj['message']
    }
  }

  streamParser.on('data', reporter)

  await installPkgs(['ajv-keywords@1.5.0'], testDefaults())

  streamParser['removeListener']('data', reporter)

  t.ok(log)
})

test('top peer dependency is not linked on subsequent install', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['ajv@4.10.4'], testDefaults())

  await installPkgs(['ajv-keywords@1.5.0'], testDefaults())

  t.ok(await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', NM, 'ajv-keywords')), 'dependent is at the normal location')
  t.ok(!await exists(path.join(NM, '.localhost+4873', 'ajv-keywords', '1.5.0', 'ajv@4.10.4', NM, 'ajv')), 'peer dependency is not linked')
})

async function okFile (t: tape.Test, filename: string) {
  t.ok(await exists(filename), `exists ${filename}`)
}

test('peer dependencies are linked', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['abc-parent-with-ab', 'abc-grand-parent-with-c', 'peer-c@2.0.0'], testDefaults())

  const pkgVariationsDir = path.join(NM, '.localhost+4873', 'abc', '1.0.0')

  const pkgVariation1 = path.join(pkgVariationsDir, 'peer-a@1.0.0+peer-b@1.0.0+peer-c@1.0.0', NM)
  await okFile(t, path.join(pkgVariation1, 'abc'))
  await okFile(t, path.join(pkgVariation1, 'peer-a'))
  await okFile(t, path.join(pkgVariation1, 'peer-b'))
  await okFile(t, path.join(pkgVariation1, 'peer-c'))
  await okFile(t, path.join(pkgVariation1, 'dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(pkgVariationsDir, 'peer-a@1.0.0+peer-b@1.0.0', NM)
  await okFile(t, path.join(pkgVariation2, 'abc'))
  await okFile(t, path.join(pkgVariation2, 'peer-a'))
  await okFile(t, path.join(pkgVariation2, 'peer-b'))
  await okFile(t, path.join(pkgVariation2, 'dep-of-pkg-with-1-dep'))

  t.equal(deepRequireCwd(['abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '2.0.0')
  t.equal(deepRequireCwd(['abc-grand-parent-with-c', 'abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version, '1.0.0')
})

test('scoped peer dependency is linked', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['for-testing-scoped-peers'], testDefaults())

  const pkgVariation = path.join(NM, '.localhost+4873', '@having', 'scoped-peer', '1.0.0', '@scoped!peer@1.0.0', NM)
  await okFile(t, path.join(pkgVariation, '@having', 'scoped-peer'))
  await okFile(t, path.join(pkgVariation, '@scoped', 'peer'))
})

test('peer bins are linked', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['for-testing-peers-having-bins'], testDefaults())

  const pkgVariation = path.join('.localhost+4873', 'pkg-with-peer-having-bin', '1.0.0', 'peer-with-bin@1.0.0', NM)

  await project.isExecutable(path.join(pkgVariation, 'pkg-with-peer-having-bin', NM, '.bin', 'peer-with-bin'))

  await project.isExecutable(path.join(pkgVariation, 'pkg-with-peer-having-bin', NM, '.bin', 'hello-world-js-bin'))
})

test('run pre/postinstall scripts of each variations of packages with peer dependencies', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['parent-of-pkg-with-events-and-peers', 'pkg-with-events-and-peers', 'peer-c@2.0.0'], testDefaults())

  const pkgVariation1 = path.join(NM, '.localhost+4873', 'pkg-with-events-and-peers', '1.0.0', 'peer-c@1.0.0', NM)
  await okFile(t, path.join(pkgVariation1, 'pkg-with-events-and-peers', 'generated-by-preinstall.js'))
  await okFile(t, path.join(pkgVariation1, 'pkg-with-events-and-peers', 'generated-by-postinstall.js'))

  const pkgVariation2 = path.join(NM, '.localhost+4873', 'pkg-with-events-and-peers', '1.0.0', NM)
  await okFile(t, path.join(pkgVariation2, 'pkg-with-events-and-peers', 'generated-by-preinstall.js'))
  await okFile(t, path.join(pkgVariation2, 'pkg-with-events-and-peers', 'generated-by-postinstall.js'))
})

test('package that resolves its own peer dependency', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['pkg-with-resolved-peer', 'peer-c@2.0.0'], testDefaults())

  t.ok(await exists(path.join(NM, '.localhost+4873', 'pkg-with-resolved-peer', '1.0.0', 'peer-c@1.0.0', NM, 'pkg-with-resolved-peer')))
})
