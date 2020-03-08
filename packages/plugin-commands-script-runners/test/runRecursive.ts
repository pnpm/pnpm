import PnpmError from '@pnpm/error'
import { filterPkgsBySelectorObjects, readProjects } from '@pnpm/filter-workspace-packages'
import { run } from '@pnpm/plugin-commands-script-runners'
import { preparePackages } from '@pnpm/prepare'
import rimraf = require('@zkochan/rimraf')
import execa = require('execa')
import fs = require('mz/fs')
import path = require('path')
import test = require('tape')
import writeYamlFile = require('write-yaml-file')
import { DEFAULT_OPTS, REGISTRY } from './utils'

test('pnpm recursive run', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-1')" | json-append ../output1.json && node -e "process.stdout.write('project-1')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-2')" | json-append ../output1.json`,
        postbuild: `node -e "process.stdout.write('project-2-postbuild')" | json-append ../output1.json`,
        prebuild: `node -e "process.stdout.write('project-2-prebuild')" | json-append ../output1.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-3')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['build'])

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])
  t.end()
})

test('pnpm recursive run concurrently', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "let i = 20;setInterval(() => {if (!--i) process.exit(0); require('json-append').append(Date.now(),'../output1.json');},50)"`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "let i = 40;setInterval(() => {if (!--i) process.exit(0); require('json-append').append(Date.now(),'../output2.json');},25)"`,
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['build'])

  const outputs1 = await import(path.resolve('output1.json')) as number[]
  const outputs2 = await import(path.resolve('output2.json')) as number[]

  t.ok(Math.max(outputs1[0], outputs2[0]) < Math.min(outputs1[outputs1.length - 1], outputs2[outputs2.length - 1]))
  t.end()
})

test('`pnpm recursive run` fails when run without filters and no package has the desired command', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    }, ['this-command-does-not-exist'])
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT')
  t.end()
})

test('`pnpm recursive run` fails when run with a filter that includes all packages and no package has the desired command', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
    },
  ])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      ...await readProjects(process.cwd(), [{ namePattern: '*' }]),
      dir: process.cwd(),
      recursive: true,
      workspaceDir: process.cwd(),
    }, ['this-command-does-not-exist'])
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT')
  t.end()
})

test('`pnpm recursive run` succeeds when run against a subset of packages and no package has the desired command', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',
    },
  ])

  const { allProjects } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(
    allProjects,
    [{ namePattern: 'project-1' }],
    { workspaceDir: process.cwd() },
  )
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['this-command-does-not-exist'])
  t.end()
})

test('testing the bail config with "pnpm recursive run"', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        build: `exit 1 && node -e "process.stdout.write('project-2')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-3')" | json-append ../output.json`,
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  let err1!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    }, ['build', '--no-bail'])
  } catch (_err) {
    err1 = _err
  }
  t.equal(err1.code, 'ERR_PNPM_RECURSIVE_FAIL')

  const outputs = await import(path.resolve('output.json')) as string[]
  t.deepEqual(outputs, ['project-1', 'project-3'], 'error skipped')

  await rimraf('./output.json')

  let err2!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    }, ['build'])
  } catch (_err) {
    err2 = _err
  }

  t.equal(err2.code, 'ERR_PNPM_RECURSIVE_FAIL')
  t.end()
})

test('pnpm recursive run with filtering', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
        postbuild: `node -e "process.stdout.write('project-2-postbuild')" | json-append ../output.json`,
        prebuild: `node -e "process.stdout.write('project-2-prebuild')" | json-append ../output.json`,
      },
    },
  ])

  const { allProjects } = await readProjects(process.cwd(), [])
  const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(
    allProjects,
    [{ namePattern: 'project-1' }],
    { workspaceDir: process.cwd() },
  )
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['build'])

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['project-1'])
  t.end()
})

test('`pnpm recursive run` should always trust the scripts', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project')" | json-append ../output.json`,
      },
    },
  ])

  const { allProjects } = await readProjects(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  process.env['npm_config_unsafe_perm'] = 'false'
  await run.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
    ...await readProjects(process.cwd(), []),
  }, ['build'])
  delete process.env['npm_config_unsafe_perm']

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['project'])
  t.end()
})
