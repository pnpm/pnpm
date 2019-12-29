import { filterPkgsBySelectorObjects, readWsPkgs } from '@pnpm/filter-workspace-packages'
import { test as testCommand } from '@pnpm/plugin-commands-script-runners'
import { preparePackages } from '@pnpm/prepare'
import execa = require('execa')
import path = require('path')
import test = require('tape')
import { DEFAULT_OPTS, REGISTRY } from './utils'

test('pnpm recursive test', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        test: `node -e "process.stdout.write('project-1')" | json-append ../output1.json && node -e "process.stdout.write('project-1')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        test: `node -e "process.stdout.write('project-2')" | json-append ../output1.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        test: `node -e "process.stdout.write('project-3')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  const { allWsPkgs, selectedWsPkgsGraph } = await readWsPkgs(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await testCommand.handler([], {
    ...DEFAULT_OPTS,
    allWsPkgs,
    dir: process.cwd(),
    recursive: true,
    selectedWsPkgsGraph,
    workspaceDir: process.cwd(),
  })

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-1', 'project-2'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])
  t.end()
})

test('`pnpm recursive test` does not fail if none of the packaegs has a test command', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1'
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1'
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  const { allWsPkgs, selectedWsPkgsGraph } = await readWsPkgs(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  await testCommand.handler([], {
    ...DEFAULT_OPTS,
    allWsPkgs,
    dir: process.cwd(),
    recursive: true,
    selectedWsPkgsGraph,
    workspaceDir: process.cwd(),
  })

  t.pass('command did not fail')
  t.end()
})

test('pnpm recursive test with filtering', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        test: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        test: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
      },
    },
  ])

  const { allWsPkgs } = await readWsPkgs(process.cwd(), [])
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await testCommand.handler([], {
    ...DEFAULT_OPTS,
    allWsPkgs,
    dir: process.cwd(),
    recursive: true,
    selectedWsPkgsGraph: await filterPkgsBySelectorObjects(
      allWsPkgs,
      [{ namePattern: 'project-1' }],
      { workspaceDir: process.cwd() },
    ),
    workspaceDir: process.cwd(),
  })

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['project-1'])
  t.end()
})
