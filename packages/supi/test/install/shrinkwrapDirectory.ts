import prepare from '@pnpm/prepare'
import loadYamlFile = require('load-yaml-file')
import path = require('path')
import { installPkgs } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)
const testSkip = promisifyTape(tape.skip)

testSkip('subsequent installation uses same shrinkwrap directory by default', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-positive@1.0.0'], await testDefaults({ shrinkwrapDirectory: path.resolve('..') }))

  await installPkgs(['is-negative@1.0.0'], await testDefaults())

  const shr = await loadYamlFile(path.resolve('..', 'shrinkwrap.yaml'))

  t.deepEqual(Object.keys(shr['packages']), ['/is-negative/1.0.0', '/is-positive/1.0.0']) // tslint:disable-line:no-string-literal
})

testSkip('subsequent installation fails if a different shrinkwrap directory is specified', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-positive@1.0.0'], await testDefaults({ shrinkwrapDirectory: path.resolve('..') }))

  let err!: Error & {code: string}

  try {
    await installPkgs(['is-negative@1.0.0'], await testDefaults({ shrinkwrapDirectory: process.cwd() }))
  } catch (_) {
    err = _
  }

  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_SHRINKWRAP_DIRECTORY_MISMATCH', 'failed with correct error code')
})
