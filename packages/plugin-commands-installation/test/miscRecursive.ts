import PnpmError from '@pnpm/error'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { Lockfile } from '@pnpm/lockfile-types'
import { add, install, remove, update } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { ProjectManifest } from '@pnpm/types'
import readYamlFile from 'read-yaml-file'
import { DEFAULT_OPTS } from './utils'
import path = require('path')
import loadJsonFile = require('load-json-file')
import fs = require('mz/fs')
import writeJsonFile = require('write-json-file')
import writeYamlFile = require('write-yaml-file')

test('recursive add/remove', async () => {
  const projects = preparePackages(undefined, [
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

  expect(projects['project-1'].requireModule('is-positive')).toBeTruthy()
  expect(projects['project-2'].requireModule('is-negative')).toBeTruthy()
  await projects['project-2'].has('is-negative')

  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['noop'])

  expect(projects['project-1'].requireModule('noop')).toBeTruthy()
  expect(projects['project-2'].requireModule('noop')).toBeTruthy()

  await remove.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-negative'])

  await projects['project-2'].hasNot('is-negative')
})

test('recursive add/remove in workspace with many lockfiles', async () => {
  const projects = preparePackages(undefined, [
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

  expect(projects['project-1'].requireModule('is-positive')).toBeTruthy()
  expect(projects['project-2'].requireModule('is-negative')).toBeTruthy()
  await projects['project-2'].has('is-negative')

  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['noop'])

  expect(projects['project-1'].requireModule('noop')).toBeTruthy()
  expect(projects['project-2'].requireModule('noop')).toBeTruthy()

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
    const manifest = await loadJsonFile<ProjectManifest>(path.resolve('project-1/package.json'))
    expect(manifest).toStrictEqual({
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        noop: '^0.2.2',
      },
    })
  }
  {
    const manifest = await loadJsonFile<ProjectManifest>(path.resolve('project-2/package.json'))
    expect(manifest).toStrictEqual({
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        noop: '^0.2.2',
      },
    })
  }
})

// Created to cover the issue described in https://github.com/pnpm/pnpm/issues/1253
test('recursive install with package that has link', async () => {
  const projects = preparePackages(undefined, [
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

  expect(projects['project-1'].requireModule('is-positive')).toBeTruthy()
  expect(projects['project-1'].requireModule('project-2/package.json')).toBeTruthy()
  expect(projects['project-2'].requireModule('is-negative')).toBeTruthy()
})

test('running `pnpm recursive` on a subset of packages', async () => {
  const projects = preparePackages(undefined, [
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
})

test('running `pnpm recursive` only for packages in subdirectories of cwd', async () => {
  const projects = preparePackages(undefined, [
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
          debug: '*',
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
})

test('recursive installation fails when installation in one of the packages fails', async () => {
  preparePackages(undefined, [
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
  expect(err.code).toBe('ERR_PNPM_FETCH_404')
})

test('second run of `recursive install` after package.json has been edited manually', async () => {
  const projects = preparePackages(undefined, [
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

  expect(projects['is-negative'].requireModule('is-positive/package.json')).toBeTruthy()
})

test('recursive --filter ignore excluded packages', async () => {
  const projects = preparePackages(undefined, [
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

  await projects['project-1'].hasNot('is-positive')
  await projects['project-2'].hasNot('is-negative')
  await projects['project-3'].hasNot('minimatch')
})

test('recursive filter multiple times', async () => {
  const projects = preparePackages(undefined, [
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

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')
  await projects['project-3'].hasNot('minimatch')
})

test('recursive install --no-bail', async () => {
  const projects = preparePackages(undefined, [
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

  expect(err.code).toBe('ERR_PNPM_RECURSIVE_FAIL')

  expect(projects['project-2'].requireModule('is-negative')).toBeTruthy()
})

test('installing with "workspace=true" should work even if link-workspace-packages is off and save-workspace-protocol is false', async () => {
  const projects = preparePackages(undefined, [
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
    expect(pkg?.dependencies).toStrictEqual({ 'project-2': 'workspace:2.0.0' })
  }
  {
    const pkg = await import(path.resolve('project-2/package.json'))
    expect(pkg.dependencies).toBeFalsy()
  }

  await projects['project-1'].has('project-2')
})

test('recursive install on workspace with custom lockfile-dir', async () => {
  preparePackages(undefined, [
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
  expect(Object.keys(lockfile.importers)).toStrictEqual(['../project-1', '../project-2'])
})

test('recursive install in a monorepo with different modules directories', async () => {
  const projects = preparePackages(undefined, [
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
})

test('prefer-workspace-package', async () => {
  await addDistTag({
    distTag: 'latest',
    package: 'foo',
    version: '100.1.0',
  })
  preparePackages(undefined, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        foo: '^100.0.0',
      },
    },
    {
      name: 'foo',
      version: '100.0.0',
    },
  ])

  await install.handler({
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    linkWorkspacePackages: true,
    preferWorkspacePackages: true,
    lockfileDir: process.cwd(),
    recursive: true,
    sharedWorkspaceLockfile: true,
    workspace: true,
    workspaceDir: process.cwd(),
  })

  const lockfile = await readYamlFile<Lockfile>(path.resolve('pnpm-lock.yaml'))
  expect(lockfile.importers['project-1'].dependencies?.foo).toBe('link:../foo')
})
