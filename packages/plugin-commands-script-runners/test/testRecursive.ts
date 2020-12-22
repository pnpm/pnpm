import { filterPkgsBySelectorObjects, readProjects } from '@pnpm/filter-workspace-packages'
import { test as testCommand } from '@pnpm/plugin-commands-script-runners'
import { preparePackages } from '@pnpm/prepare'
import { DEFAULT_OPTS, REGISTRY } from './utils'
import path = require('path')
import execa = require('execa')

test('pnpm recursive test', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        test: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output1.json && node -e "process.stdout.write(\'project-1\')" | json-append ../output2.json',
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
        test: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output1.json',
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
        test: 'node -e "process.stdout.write(\'project-3\')" | json-append ../output2.json',
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
  await testCommand.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  const { default: outputs1 } = await import(path.resolve('output1.json'))
  const { default: outputs2 } = await import(path.resolve('output2.json'))

  expect(outputs1).toStrictEqual(['project-1', 'project-2'])
  expect(outputs2).toStrictEqual(['project-1', 'project-3'])
})

test('`pnpm recursive test` does not fail if none of the packages has a test command', async () => {
  preparePackages([
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

  await testCommand.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })
})

test('pnpm recursive test with filtering', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        test: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output.json',
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
        test: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output.json',
      },
    },
  ])

  const { allProjects } = await readProjects(process.cwd(), [])
  const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(
    allProjects,
    [{ namePattern: 'project-1' }],
    { workspaceDir: process.cwd() }
  )
  await execa('pnpm', [
    'install',
    '-r',
    '--registry',
    REGISTRY,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await testCommand.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  const { default: outputs } = await import(path.resolve('output.json'))

  expect(outputs).toStrictEqual(['project-1'])
})
