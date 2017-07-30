import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import writePkg = require('write-pkg')
import {
  prepare,
  testDefaults,
  execPnpm,
  isExecutable,
 } from './utils'
import thenify = require('thenify')
import fs = require('mz/fs')

test('linking multiple packages', async (t: tape.Test) => {
  const project = prepare(t)

  process.chdir('..')
  process.env.NPM_CONFIG_PREFIX = path.resolve('global')

  await writePkg('linked-foo', {name: 'linked-foo', version: '1.0.0'})
  await writePkg('linked-bar', {name: 'linked-bar', version: '1.0.0'})

  process.chdir('linked-foo')

  t.comment('linking linked-foo to global package')
  await execPnpm('link')

  process.chdir('..')
  process.chdir('project')

  await execPnpm('link', 'linked-foo', '../linked-bar')

  project.has('linked-foo')
  project.has('linked-bar')
})

test('link global bin', async function (t: tape.Test) {
  prepare(t)
  process.chdir('..')

  const global = path.resolve('global')
  process.env.NPM_CONFIG_PREFIX = global

  await writePkg('package-with-bin', {name: 'package-with-bin', version: '1.0.0', bin: 'bin.js'})
  await fs.writeFile('package-with-bin/bin.js', '#!/usr/bin/env node\nconsole.log(/hi/)\n', 'utf8')

  process.chdir('package-with-bin')

  await execPnpm('link')

  isExecutable(t, path.join(global, 'bin', 'package-with-bin'))
})
