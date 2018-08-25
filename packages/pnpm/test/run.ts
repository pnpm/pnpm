import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  execPnpmSync,
  prepare,
} from './utils'

const test = promisifyTape(tape)

test('pnpm run: returns correct exit code', async (t: tape.Test) => {
  const project = prepare(t, {
    scripts: {
      exit0: 'exit 0',
      exit1: 'exit 1',
    },
  })

  t.equal(execPnpmSync('run', 'exit0').status, 0)
  t.equal(execPnpmSync('run', 'exit1').status, 1)
})

test('pass the args to the command that is specfied in the build script', async t => {
  prepare(t, {
    scripts: {
      test: 'ts-node test'
    },
  })

  const result = execPnpmSync('run', 'test', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})
