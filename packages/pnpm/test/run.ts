import prepare, { preparePackages } from '@pnpm/prepare'
import promisifyTape from 'tape-promise'
import { execPnpm, execPnpmSync } from './utils'
import path = require('path')
import fs = require('mz/fs')
import tape = require('tape')

const test = promisifyTape(tape)

const RECORD_ARGS_FILE = 'require(\'fs\').writeFileSync(\'args.json\', JSON.stringify(require(\'./args.json\').concat([process.argv.slice(2)])), \'utf8\')'

test('run -r: pass the args to the command that is specfied in the build script', async (t: tape.Test) => {
  preparePackages(t, [{
    name: 'project',
    scripts: {
      foo: 'node recordArgs',
      postfoo: 'node recordArgs',
      prefoo: 'node recordArgs',
    },
  }])
  await fs.writeFile('project/args.json', '[]', 'utf8')
  await fs.writeFile('project/recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await execPnpm(['run', '-r', 'foo', 'arg', '--', '--flag=true'])

  const args = await import(path.resolve('project/args.json'))
  t.deepEqual(args, [
    [],
    ['arg', '--flag=true'],
    [],
  ])
})

test('test -r: pass the args to the command that is specfied in the build script of a package.json manifest', async (t: tape.Test) => {
  preparePackages(t, [{
    name: 'project',
    scripts: {
      test: 'ts-node test',
    },
  }])

  const result = execPnpmSync(['test', '-r', 'arg', '--', '--flag=true'])

  t.ok((result.stdout as Buffer).toString('utf8').match(/ts-node test "arg" "--flag=true"/), 'command was successful')
})

test('start: run "node server.js" by default', async (t: tape.Test) => {
  prepare(t, {}, { manifestFormat: 'YAML' })

  await fs.writeFile('server.js', 'console.log("Hello world!")', 'utf8')

  const result = execPnpmSync(['start'])

  t.ok((result.stdout as Buffer).toString('utf8').match(/Hello world!/), 'command was successful')
})

test('install-test: install dependencies and runs tests', async (t: tape.Test) => {
  prepare(t, {
    dependencies: {
      'json-append': '1',
    },
    scripts: {
      posttest: 'node -e "process.stdout.write(\'posttest\')" | json-append ./output.json',
      pretest: 'node -e "process.stdout.write(\'pretest\')" | json-append ./output.json',
      test: 'node -e "process.stdout.write(\'test\')" | json-append ./output.json',
    },
  }, { manifestFormat: 'JSON5' })

  await execPnpm(['install-test'])

  const scriptsRan = await import(path.resolve('output.json'))
  t.deepEqual(scriptsRan, [
    'pretest',
    'test',
    'posttest',
  ])
})

test('silent run only prints the output of the child process', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      hi: 'echo hi && exit 1',
    },
  })

  const result = execPnpmSync(['run', '--silent', 'hi'])

  t.ok(result.stdout.toString().trim() === 'hi')
})
