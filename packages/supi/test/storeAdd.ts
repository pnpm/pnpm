import {
  storeAdd,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {prepare, testDefaults} from './utils'

const test = promisifyTape(tape)

test('add packages to the store', async (t: tape.Test) => {
  const project = prepare(t)

  const opts = await testDefaults()
  opts.prefix = ''
  await storeAdd(['express@4.16.3'], opts as any) // tslint:disable-line

  await project.storeHas('express', '4.16.3')
})
