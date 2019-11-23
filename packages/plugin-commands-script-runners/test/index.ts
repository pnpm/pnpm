import {
  restart,
  run,
  start,
  stop,
  test as testCommand,
} from '@pnpm/plugin-commands-script-runners'
import prepare from '@pnpm/prepare'
import { stripIndent } from 'common-tags'
import execa = require('execa')
import fs = require('mz/fs')
import path = require('path')
import test = require('tape')

test('pnpm run: returns correct exit code', async (t) => {
  prepare(t, {
    scripts: {
      exit0: 'exit 0',
      exit1: 'exit 1',
    },
  })

  await run.handler(['exit0'], {
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  })

  let err!: Error & { errno: Number }
  try {
    await run.handler(['exit1'], {
      dir: process.cwd(),
      extraBinPaths: [],
      rawConfig: {},
    })
  } catch (_err) {
    err = _err
  }
  t.equal(err.errno, 1)

  t.end()
})

const RECORD_ARGS_FILE = `require('fs').writeFileSync('args.json', JSON.stringify(require('./args.json').concat([process.argv.slice(2)])), 'utf8')`

test('run: pass the args to the command that is specfied in the build script', async (t) => {
  prepare(t, {
    scripts: {
      foo: 'node recordArgs',
      postfoo: 'node recordArgs',
      prefoo: 'node recordArgs',
    },
  })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await run.handler(['foo', 'arg', '--flag=true', '--help', '-h'], {
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  })

  const args = await import(path.resolve('args.json'))
  t.deepEqual(args, [
    [],
    ['arg', '--flag=true', '--help', '-h'],
    [],
  ])

  t.end()
})

test('run: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t) => {
  prepare(t, {
    scripts: {
      foo: 'node recordArgs',
      postfoo: 'node recordArgs',
      prefoo: 'node recordArgs',
    },
  }, { manifestFormat: 'YAML' })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await run.handler(['foo', 'arg', '--flag=true', '--help', '-h'], {
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  })

  const args = await import(path.resolve('args.json'))
  t.deepEqual(args, [
    [],
    ['arg', '--flag=true', '--help', '-h'],
    [],
  ])

  t.end()
})

test('test: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t) => {
  prepare(t, {
    scripts: {
      posttest: 'node recordArgs',
      pretest: 'node recordArgs',
      test: 'node recordArgs',
    },
  }, { manifestFormat: 'YAML' })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await testCommand.handler(['arg', '--flag=true', '--help', '-h'], {
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  })

  const args = await import(path.resolve('args.json'))
  t.deepEqual(args, [
    [],
    ['arg', '--flag=true', '--help', '-h'],
    [],
  ])

  t.end()
})

test('start: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t) => {
  prepare(t, {
    scripts: {
      poststart: 'node recordArgs',
      prestart: 'node recordArgs',
      start: 'node recordArgs',
    },
  }, { manifestFormat: 'YAML' })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await start.handler(['arg', '--flag=true', '--help', '-h'], {
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  })

  const args = await import(path.resolve('args.json'))
  t.deepEqual(args, [
    [],
    ['arg', '--flag=true', '--help', '-h'],
    [],
  ])

  t.end()
})

test('stop: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t) => {
  prepare(t, {
    scripts: {
      poststop: 'node recordArgs',
      prestop: 'node recordArgs',
      stop: 'node recordArgs',
    },
  }, { manifestFormat: 'YAML' })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await stop.handler(['arg', '--flag=true', '--help', '-h'], {
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  })

  const args = await import(path.resolve('args.json'))
  t.deepEqual(args, [
    [],
    ['arg', '--flag=true', '--help', '-h'],
    [],
  ])

  t.end()
})

test('restart: run stop, restart and start', async (t) => {
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

  await execa('pnpm', ['add', 'json-append@1'])
  await restart.handler([], {
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  })

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
  t.end()
})

test('"pnpm run" prints the list of available commands', async (t) => {
  prepare(t, {
    scripts: {
      foo: 'echo hi',
      test: 'ts-node test',
    },
  })

  const output = await run.handler([], {
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  })

  t.equal(output, stripIndent`
    Lifecycle scripts:
      test
        ts-node test

    Commands available via "pnpm run":
      foo
        echo hi`,
  )
  t.end()
})
