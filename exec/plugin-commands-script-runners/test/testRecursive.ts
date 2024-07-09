import path from 'path'
import { filterPkgsBySelectorObjects } from '@pnpm/filter-workspace-packages'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { test as testCommand } from '@pnpm/plugin-commands-script-runners'
import { preparePackages } from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import execa from 'execa'
import { DEFAULT_OPTS, REGISTRY_URL } from './utils'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

test('pnpm recursive test', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        test: `${server1.sendLineScript('project-1')} && ${server2.sendLineScript('project-1')}`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        test: server1.sendLineScript('project-2'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        test: server2.sendLineScript('project-3'),
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await execa('node', [
    pnpmBin,
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
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

  expect(server1.getLines()).toStrictEqual(['project-1', 'project-2'])
  expect(server2.getLines()).toStrictEqual(['project-1', 'project-3'])
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

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await execa('node', [
    pnpmBin,
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
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
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        test: server.sendLineScript('project-1'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        test: server.sendLineScript('project-2'),
      },
    },
  ])

  const { allProjects } = await filterPackagesFromDir(process.cwd(), [])
  const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(
    allProjects,
    [{ namePattern: 'project-1' }],
    { workspaceDir: process.cwd() }
  )
  await execa('node', [
    pnpmBin,
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
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

  expect(server.getLines()).toStrictEqual(['project-1'])
})
