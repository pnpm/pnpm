import prepare, { prepareWithYamlManifest } from '@pnpm/prepare'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpmSync } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('pnpm run: returns correct exit code', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      exit0: 'exit 0',
      exit1: 'exit 1',
    },
  })

  t.equal(execPnpmSync('run', 'exit0').status, 0)
  t.equal(execPnpmSync('run', 'exit1').status, 1)
})

test('run: pass the args to the command that is specfied in the build script', async t => {
  prepare(t, {
    scripts: {
      foo: 'ts-node test'
    },
  })

  const result = execPnpmSync('run', 'foo', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})

test('run: pass the args to the command that is specfied in the build script of a package.yaml manifest', async t => {
  prepareWithYamlManifest(t, {
    scripts: {
      foo: 'ts-node test'
    },
  })

  const result = execPnpmSync('run', 'foo', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})

test('test: pass the args to the command that is specfied in the build script of a package.yaml manifest', async t => {
  prepareWithYamlManifest(t, {
    scripts: {
      test: 'ts-node test'
    },
  })

  const result = execPnpmSync('test', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})

test('start: pass the args to the command that is specfied in the build script of a package.yaml manifest', async t => {
  prepareWithYamlManifest(t, {
    scripts: {
      start: 'ts-node test'
    },
  })

  const result = execPnpmSync('start', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})
