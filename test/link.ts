import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import writePkg = require('write-pkg')
import {
  prepare,
  testDefaults,
 } from './utils'
import thenify = require('thenify')
import {link} from '../src/cmd'

test('linking multiple packages', async (t: tape.Test) => {
  const project = prepare(t)

  process.chdir('..')
  const globalPrefix = path.resolve('global')

  await writePkg('linked-foo', {name: 'linked-foo', version: '1.0.0'})
  await writePkg('linked-bar', {name: 'linked-bar', version: '1.0.0'})

  process.chdir('linked-foo')

  const opts = Object.assign(testDefaults(), {globalPrefix})

  t.comment('linking linked-foo to global package')
  await link([], opts)

  process.chdir('..')
  process.chdir('project')

  await link(['linked-foo', '../linked-bar'], opts)

  project.has('linked-foo')
  project.has('linked-bar')
})
