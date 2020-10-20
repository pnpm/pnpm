/// <reference path="../../../typings/index.d.ts" />
import PnpmError from '@pnpm/error'
import { readProjects } from '@pnpm/filter-workspace-packages'
import {
  restart,
  run,
  test as testCommand,
} from '@pnpm/plugin-commands-script-runners'
import prepare, { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import './exec'
import './runCompletion'
import './runRecursive'
import './testRecursive'
import { DEFAULT_OPTS, REGISTRY } from './utils'
import execa = require('execa')
import fs = require('mz/fs')
import path = require('path')
import test = require('tape')
import writeYamlFile = require('write-yaml-file')

const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.js')

test('pnpm run: returns correct exit code', async (t) => {
  prepare(t, {
    scripts: {
      exit0: 'exit 0',
      exit1: 'exit 1',
    },
  })

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  }, ['exit0'])

  let err!: Error & { errno: Number }
  try {
    await run.handler({
      dir: process.cwd(),
      extraBinPaths: [],
      rawConfig: {},
    }, ['exit1'])
  } catch (_err) {
    err = _err
  }
  t.equal(err.errno, 1)

  t.end()
})

const RECORD_ARGS_FILE = 'require(\'fs\').writeFileSync(\'args.json\', JSON.stringify(require(\'./args.json\').concat([process.argv.slice(2)])), \'utf8\')'

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

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  }, ['foo', 'arg', '--flag=true', '--help', '-h'])

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

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  }, ['foo', 'arg', '--flag=true', '--help', '-h'])

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

  await testCommand.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  }, ['arg', '--flag=true', '--help', '-h'])

  const args = await import(path.resolve('args.json'))
  t.deepEqual(args, [
    [],
    ['arg', '--flag=true', '--help', '-h'],
    [],
  ])

  t.end()
})

test('run start: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t) => {
  prepare(t, {
    scripts: {
      poststart: 'node recordArgs',
      prestart: 'node recordArgs',
      start: 'node recordArgs',
    },
  }, { manifestFormat: 'YAML' })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  }, ['start', 'arg', '--flag=true', '--help', '-h'])

  const args = await import(path.resolve('args.json'))
  t.deepEqual(args, [
    [],
    ['arg', '--flag=true', '--help', '-h'],
    [],
  ])

  t.end()
})

test('run stop: pass the args to the command that is specfied in the build script of a package.yaml manifest', async (t) => {
  prepare(t, {
    scripts: {
      poststop: 'node recordArgs',
      prestop: 'node recordArgs',
      stop: 'node recordArgs',
    },
  }, { manifestFormat: 'YAML' })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  }, ['stop', 'arg', '--flag=true', '--help', '-h'])

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
      poststop: 'node -e "process.stdout.write(\'poststop\')" | json-append ./output.json',
      prestop: 'node -e "process.stdout.write(\'prestop\')" | json-append ./output.json',
      stop: 'node -e "process.stdout.write(\'stop\')" | json-append ./output.json',

      postrestart: 'node -e "process.stdout.write(\'postrestart\')" | json-append ./output.json',
      prerestart: 'node -e "process.stdout.write(\'prerestart\')" | json-append ./output.json',
      restart: 'node -e "process.stdout.write(\'restart\')" | json-append ./output.json',

      poststart: 'node -e "process.stdout.write(\'poststart\')" | json-append ./output.json',
      prestart: 'node -e "process.stdout.write(\'prestart\')" | json-append ./output.json',
      start: 'node -e "process.stdout.write(\'start\')" | json-append ./output.json',
    },
  })

  await execa('pnpm', ['add', 'json-append@1'])
  await restart.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  }, [])

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

  const output = await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  }, [])

  t.equal(output, `\
Lifecycle scripts:
  test
    ts-node test

Commands available via "pnpm run":
  foo
    echo hi`)
  t.end()
})

test('"pnpm run" prints the list of available commands, including commands of the root workspace project', async (t) => {
  preparePackages(t, [
    {
      location: '.',
      package: {
        dependencies: {
          'json-append': '1',
        },
        scripts: {
          build: 'echo root',
          test: 'test-all',
        },
      },
    },
    {
      name: 'foo',
      version: '1.0.0',

      scripts: {
        foo: 'echo hi',
        test: 'ts-node test',
      },
    },
  ])
  await writeYamlFile('pnpm-workspace.yaml', {})
  const workspaceDir = process.cwd()

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])

  process.chdir('foo')
  const output = await run.handler({
    allProjects,
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
    selectedProjectsGraph,
    workspaceDir,
  }, [])

  t.equal(output, `\
Lifecycle scripts:
  test
    ts-node test

Commands available via "pnpm run":
  foo
    echo hi

Commands of the root workspace project (to run them, use "pnpm -w run"):
  build
    echo root
  test
    test-all`)
  t.end()
})

test('pnpm run does not fail with --if-present even if the wanted script is not present', async (t) => {
  prepare(t, {})

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    ifPresent: true,
    rawConfig: {},
  }, ['build'])

  t.end()
})

test('if a script is not found but is present in the root, print an info message about it in the error message', async (t) => {
  preparePackages(t, [
    {
      location: '.',
      package: {
        dependencies: {
          'json-append': '1',
        },
        scripts: {
          build: 'node -e "process.stdout.write(\'root\')" | json-append ./output.json',
        },
      },
    },
    {
      name: 'foo',
      version: '1.0.0',
    },
  ])
  await writeYamlFile('pnpm-workspace.yaml', {})

  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: path.resolve('foo'),
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    }, ['build'])
  } catch (_err) {
    err = _err
  }

  t.ok(err)
  t.ok(err.hint.includes('But build is present in the root'))
  t.end()
})

test('scripts work with PnP', async (t) => {
  prepare(t, {
    scripts: {
      foo: 'node -e "process.stdout.write(\'foo\')" | json-append ./output.json',
    },
  })

  await execa(pnpmBin, ['add', 'json-append@1'], {
    env: {
      NPM_CONFIG_NODE_LINKER: 'pnp',
      NPM_CONFIG_SYMLINK: 'false',
    },
  })
  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
  }, ['foo'])

  const scriptsRan = await import(path.resolve('output.json'))
  t.deepEqual(scriptsRan, ['foo'])
  t.end()
})

test('pnpm run with custom shell', async (t) => {
  prepare(t, {
    scripts: {
      build: 'foo bar',
    },
    dependencies: {
      'shell-mock': '0.0.0',
    },
  })

  await execa(pnpmBin, [
    'install',
    `--registry=http://localhost:${REGISTRY_MOCK_PORT}`,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    rawConfig: {},
    scriptShell: path.resolve('node_modules/.bin/shell-mock'),
  }, ['build'])

  t.deepEqual(await import(path.resolve('shell-input.json')), ['-c', 'foo bar'])
  t.end()
})
