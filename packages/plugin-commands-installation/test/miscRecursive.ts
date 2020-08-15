import PnpmError from '@pnpm/error'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { Lockfile } from '@pnpm/lockfile-types'
import { add, install, remove, update } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import fs = require('mz/fs')
import path = require('path')
import readYamlFile from 'read-yaml-file'
import test = require('tape')
import writeJsonFile = require('write-json-file')
import writeYamlFile = require('write-yaml-file')
import { DEFAULT_OPTS } from './utils'

test('recursive add/remove', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))
  await projects['project-2'].has('is-negative')

  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['noop'])

  t.ok(projects['project-1'].requireModule('noop'))
  t.ok(projects['project-2'].requireModule('noop'))

  await remove.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-negative'])

  await projects['project-2'].hasNot('is-negative')

  t.end()
})

test('recursive add/remove in workspace with many lockfiles', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: false,
    workspaceDir: process.cwd(),
  })

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))
  await projects['project-2'].has('is-negative')

  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['noop'])

  t.ok(projects['project-1'].requireModule('noop'))
  t.ok(projects['project-2'].requireModule('noop'))

  await remove.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-negative'])

  await projects['project-2'].hasNot('is-negative')

  {
    const manifest = await import(path.resolve('project-1/package.json'))
    t.deepEqual(manifest, {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'noop': '^0.2.2',
      },
    })
  }
  {
    const manifest = await import(path.resolve('project-2/package.json'))
    t.deepEqual(manifest, {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'noop': '^0.2.2',
      },
    })
  }

  t.end()
})

// Created to cover the issue described in https://github.com/pnpm/pnpm/issues/1253
test('recursive install with package that has link', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': 'link:../project-2',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await install.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-1'].requireModule('project-2/package.json'))
  t.ok(projects['project-2'].requireModule('is-negative'))
  t.end()
})

test('running `pnpm recursive` on a subset of packages', async t => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['project-1'] })

  await install.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  await projects['project-1'].has('is-positive')
  await projects['project-2'].hasNot('is-negative')
  t.end()
})

test('running `pnpm recursive` only for packages in subdirectories of cwd', async t => {
  const projects = preparePackages(t, [
    {
      location: 'packages/project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
    {
      location: 'packages/project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
    },
    {
      location: 'root-project',
      package: {
        name: 'root-project',
        version: '1.0.0',

        dependencies: {
          'debug': '*',
        },
      },
    },
  ])

  await fs.mkdir('node_modules')
  process.chdir('packages')

  await install.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')
  await projects['root-project'].hasNot('debug')
  t.end()
})

test('recursive installation fails when installation in one of the packages fails', async t => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'this-pkg-does-not-exist': '100.100.100',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  let err!: PnpmError
  try {
    await install.handler({
      ...DEFAULT_OPTS,
      ...await readProjects(process.cwd(), []),
      dir: process.cwd(),
      recursive: true,
      workspaceDir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_FETCH_404')
  t.end()
})

test('second run of `recursive install` after package.json has been edited manually', async t => {
  const projects = preparePackages(t, [
    {
      name: 'is-negative',
      version: '1.0.0',

      dependencies: {
        'is-positive': '2.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '1.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  await writeJsonFile('is-negative/package.json', {
    name: 'is-negative',
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  t.ok(projects['is-negative'].requireModule('is-positive/package.json'))
  t.end()
})

test('recursive --filter ignore excluded packages', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        minimatch: '*',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', {
    packages: [
      '**',
      '!project-1',
    ],
  })

  await install.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), [
      { includeDependencies: true, namePattern: 'project-1' },
    ]),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  projects['project-1'].hasNot('is-positive')
  projects['project-2'].hasNot('is-negative')
  projects['project-3'].hasNot('minimatch')
  t.end()
})

test('recursive filter multiple times', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        minimatch: '*',
      },
    },
  ])

  await install.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), [
      { namePattern: 'project-1' },
      { namePattern: 'project-2' },
    ]),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
  t.end()
})

test('recursive install --no-bail', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm/this-does-not-exist': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  let err!: PnpmError
  try {
    await install.handler({
      ...DEFAULT_OPTS,
      ...await readProjects(process.cwd(), []),
      bail: false,
      dir: process.cwd(),
      recursive: true,
      workspaceDir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }

  t.equal(err.code, 'ERR_PNPM_RECURSIVE_FAIL')

  t.ok(projects['project-2'].requireModule('is-negative'))
  t.end()
})

test('installing with "workspace=true" should work even if link-workspace-packages is off and save-workspace-protocol is false', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'project-2': '0.0.0',
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await update.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    linkWorkspacePackages: false,
    lockfileDir: process.cwd(),
    recursive: true,
    saveWorkspaceProtocol: false,
    sharedWorkspaceLockfile: true,
    workspace: true,
    workspaceDir: process.cwd(),
  }, ['project-2'])

  {
    const pkg = await import(path.resolve('project-1/package.json'))
    t.deepEqual(pkg && pkg.dependencies, { 'project-2': 'workspace:2.0.0' })
  }
  {
    const pkg = await import(path.resolve('project-2/package.json'))
    t.notOk(pkg.dependencies)
  }

  await projects['project-1'].has('project-2')

  t.end()
})

test('recursive install on workspace with custom lockfile-dir', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const lockfileDir = path.resolve('_')
  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    lockfileDir,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  const lockfile = await readYamlFile<Lockfile>(path.join(lockfileDir, 'pnpm-lock.yaml'))
  t.deepEqual(Object.keys(lockfile.importers), ['../project-1', '../project-2'])

  t.end()
})

test('recursive install in a monorepo with different modules directories', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])
  await fs.writeFile('project-1/.npmrc', 'modules-dir=modules_1', 'utf8')
  await fs.writeFile('project-2/.npmrc', 'modules-dir=modules_2', 'utf8')

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  await projects['project-1'].has('is-positive', 'modules_1')
  await projects['project-2'].has('is-positive', 'modules_2')

  t.end()
})
