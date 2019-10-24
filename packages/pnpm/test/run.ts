import prepare, { preparePackages } from '@pnpm/prepare'
import { stripIndent } from 'common-tags'
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

const RECORD_ARGS_FILE = `require('fs').writeFileSync('args.json', JSON.stringify(require('./args.json').concat([process.argv.slice(2)])), 'utf8')`

test('run: pass the args to the command that is specfied in the build script', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      foo: 'node recordArgs',
      prefoo: 'node recordArgs',
      postfoo: 'node recordArgs',
    },
  })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await execPnpm('run', 'foo', 'arg', '--', '--flag=true', '--help', '-h')

  const args = await import(path.resolve('args.json'))
  t.deepEqual(args, [
    [],
    ['arg', '--flag=true', '--help', '-h'],
    [],
  ])
})

test('run -r: pass the args to the command that is specfied in the build script', async (t: tape.Test) => {
  preparePackages(t, [{
    name: 'project',
    scripts: {
      foo: 'node recordArgs',
      prefoo: 'node recordArgs',
      postfoo: 'node recordArgs',
    },
  }])
  await fs.writeFile('project/args.json', '[]', 'utf8')
  await fs.writeFile('project/recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await execPnpm('run', '-r', 'foo', 'arg', '--', '--flag=true')

  const args = await import(path.resolve('project/args.json'))
  t.deepEqual(args, [
    [],
    ['arg', '--flag=true'],
    [],
  ])
})

test('run: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      foo: 'ts-node test'
    },
  }, { manifestFormat: 'YAML' })

  const result = execPnpmSync('run', 'foo', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})

test('test: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      test: 'ts-node test'
    },
  }, { manifestFormat: 'YAML' })

  const result = execPnpmSync('test', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})

test('test -r: pass the args to the command that is specfied in the build script of a package.json manifest', async (t: tape.Test) => {
  preparePackages(t, [{
    name: 'project',
    scripts: {
      test: 'ts-node test'
    },
  }])

  const result = execPnpmSync('test', '-r', 'arg', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "arg" "--flag=true"/), 'command was successful')
})

test('start: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      start: 'ts-node test'
    },
  }, { manifestFormat: 'YAML' })

  const result = execPnpmSync('start', '--', '--flag=true')

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "--flag=true"/), 'command was successful')
})

test('start: run "node server.js" by default', async (t: tape.Test) => {
  prepare(t, {}, { manifestFormat: 'YAML' })

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
      poststop: `node -e "process.stdout.write('poststop')" | json-append ./output.json`,
      prestop: `node -e "process.stdout.write('prestop')" | json-append ./output.json`,
      stop: `node -e "process.stdout.write('stop')" | json-append ./output.json`,

      postrestart: `node -e "process.stdout.write('postrestart')" | json-append ./output.json`,
      prerestart: `node -e "process.stdout.write('prerestart')" | json-append ./output.json`,
      restart: `node -e "process.stdout.write('restart')" | json-append ./output.json`,

      poststart: `node -e "process.stdout.write('poststart')" | json-append ./output.json`,
      prestart: `node -e "process.stdout.write('prestart')" | json-append ./output.json`,
      start: `node -e "process.stdout.write('start')" | json-append ./output.json`,
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

test('install-test: install dependencies and runs tests', async (t: tape.Test) => {
  prepare(t, {
    dependencies: {
      'json-append': '1',
    },
    scripts: {
      posttest: `node -e "process.stdout.write('posttest')" | json-append ./output.json`,
      pretest: `node -e "process.stdout.write('pretest')" | json-append ./output.json`,
      test: `node -e "process.stdout.write('test')" | json-append ./output.json`,
    },
  }, { manifestFormat: 'JSON5' })

  await execPnpm('install-test')

  const scriptsRan = await import(path.resolve('output.json'))
  t.deepEqual(scriptsRan, [
    'pretest',
    'test',
    'posttest',
  ])
})

test('"pnpm run" prints the list of available commands', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      foo: 'echo hi',
      test: 'ts-node test',
    },
  })

  const result = execPnpmSync('run')

  t.equal((result.stdout as Buffer).toString('utf8'), stripIndent`
    Lifecycle scripts:
      test
        ts-node test

    Commands available via "pnpm run":
      foo
        echo hi` + '\n',
  )
})
