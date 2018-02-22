import tape = require('tape')
import promisifyTape from 'tape-promise'

import {
  execPnpm,
  prepare,
} from '../utils'

const test = promisifyTape(tape)

test('shamefully flatten the dependency tree', async function (t) {
  const project = prepare(t)

  await execPnpm('install', '--shamefully-flatten', 'express@4.16.2')

  await project.has('express')
  await project.has('debug')
  await project.has('cookie')

  await execPnpm('uninstall', '--shamefully-flatten', 'express')

  await project.hasNot('express')
  await project.hasNot('debug')
  await project.hasNot('cookie')
})
