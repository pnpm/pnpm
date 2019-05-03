import prepare, { prepareWithYamlManifest } from '@pnpm/prepare'
import fs = require('mz/fs')
import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpm, execPnpmSync } from './utils'

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

test('run: pass the args to the command that is specfied in the build script', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      foo: 'ts-node test'
    },
  })

  const result = execPnpmSync('run', 'foo', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})

test('run: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t: tape.Test) => {
  prepareWithYamlManifest(t, {
    scripts: {
      foo: 'ts-node test'
    },
  })

  const result = execPnpmSync('run', 'foo', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})

test('test: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t: tape.Test) => {
  prepareWithYamlManifest(t, {
    scripts: {
      test: 'ts-node test'
    },
  })

  const result = execPnpmSync('test', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})

test('start: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t: tape.Test) => {
  prepareWithYamlManifest(t, {
    scripts: {
      start: 'ts-node test'
    },
  })

  const result = execPnpmSync('start', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})

test('start: run "node server.js" by default', async (t: tape.Test) => {
  prepareWithYamlManifest(t)

  await fs.writeFile('server.js', 'console.log("Hello world!")', 'utf8')

  const result = execPnpmSync('start')

  t.ok((result.stdout as Buffer).toString('utf8').match(/Hello world!/), 'command was successful')
})

test('stop: pass the args to the command that is specfied in the build script', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      stop: 'ts-node test'
    },
  })

  const result = execPnpmSync('stop', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})

test('restart: run stop, restart and start', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      prestop: `node -e "process.stdout.write('prestop')" | json-append ./output.json`,
      stop: `node -e "process.stdout.write('stop')" | json-append ./output.json`,
      poststop: `node -e "process.stdout.write('poststop')" | json-append ./output.json`,

      prerestart: `node -e "process.stdout.write('prerestart')" | json-append ./output.json`,
      restart: `node -e "process.stdout.write('restart')" | json-append ./output.json`,
      postrestart: `node -e "process.stdout.write('postrestart')" | json-append ./output.json`,

      prestart: `node -e "process.stdout.write('prestart')" | json-append ./output.json`,
      start: `node -e "process.stdout.write('start')" | json-append ./output.json`,
      poststart: `node -e "process.stdout.write('poststart')" | json-append ./output.json`,
    },
  })

  await execPnpm('add', 'json-append@1')
  await execPnpm('restart')

  const scriptsRan = await import(path.resolve('output.json'))
  t.deepEqual(scriptsRan, [
    'prestop',
    'stop',
    'poststop',
    'prerestart',
    'restart',
    'postrestart',
    'prestart',
    'start',
    'poststart',
  ])
})
