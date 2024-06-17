import fs from 'fs'
import path from 'path'
import { type PnpmError } from '@pnpm/error'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { type LockfileFile } from '@pnpm/lockfile-types'
import { add, install, remove, update } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { type ProjectManifest } from '@pnpm/types'
import { sync as readYamlFile } from 'read-yaml-file'
import loadJsonFile from 'load-json-file'
import writeJsonFile from 'write-json-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import { DEFAULT_OPTS } from './utils'
import symlinkDir from 'symlink-dir'

test('recursive add/remove', async () => {
  const projects = preparePackages([
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

  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  expect(projects['project-1'].requireModule('is-positive')).toBeTruthy()
  expect(projects['project-2'].requireModule('is-negative')).toBeTruthy()
  projects['project-2'].has('is-negative')

  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
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
    allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-negative'])

  projects['project-2'].hasNot('is-negative')
})

test('recursive add/remove in workspace with many lockfiles', async () => {
  const projects = preparePackages([
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

  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: false,
    workspaceDir: process.cwd(),
  })

  expect(projects['project-1'].requireModule('is-positive')).toBeTruthy()
  expect(projects['project-2'].requireModule('is-negative')).toBeTruthy()
  projects['project-2'].has('is-negative')

  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['noop@^0.2.2'])

  expect(projects['project-1'].requireModule('noop')).toBeTruthy()
  expect(projects['project-2'].requireModule('noop')).toBeTruthy()

  await remove.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-negative'])

  projects['project-2'].hasNot('is-negative')

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
  const projects = preparePackages([
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
    ...await filterPackagesFromDir(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  expect(projects['project-1'].requireModule('is-positive')).toBeTruthy()
  expect(projects['project-1'].requireModule('project-2/package.json')).toBeTruthy()
  expect(projects['project-2'].requireModule('is-negative')).toBeTruthy()
})

test('running `pnpm recursive` on a subset of packages', async () => {
  const projects = preparePackages([
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

  writeYamlFile('pnpm-workspace.yaml', { packages: ['project-1'] })

  await install.handler({
    ...DEFAULT_OPTS,
    ...await filterPackagesFromDir(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  projects['project-1'].has('is-positive')
  projects['project-2'].hasNot('is-negative')
})

test('running `pnpm recursive` only for packages in subdirectories of cwd', async () => {
  const projects = preparePackages([
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

  fs.mkdirSync('node_modules')
  process.chdir('packages')

  await install.handler({
    ...DEFAULT_OPTS,
    ...await filterPackagesFromDir(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['root-project'].hasNot('debug')
})

test('recursive installation fails when installation in one of the packages fails', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/this-pkg-does-not-exist': '100.100.100',
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
      ...await filterPackagesFromDir(process.cwd(), []),
      dir: process.cwd(),
      recursive: true,
      workspaceDir: process.cwd(),
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_FETCH_404')
})

test('second run of `recursive install` after package.json has been edited manually', async () => {
  const projects = preparePackages([
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

  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  writeJsonFile.sync('is-negative/package.json', {
    name: 'is-negative',
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  expect(projects['is-negative'].requireModule('is-positive/package.json')).toBeTruthy()
})

test('recursive --filter ignore excluded packages', async () => {
  const projects = preparePackages([
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

  writeYamlFile('pnpm-workspace.yaml', {
    packages: [
      '**',
      '!project-1',
    ],
  })

  await install.handler({
    ...DEFAULT_OPTS,
    ...await filterPackagesFromDir(process.cwd(), [
      { includeDependencies: true, namePattern: 'project-1' },
    ]),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  projects['project-1'].hasNot('is-positive')
  projects['project-2'].hasNot('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('recursive filter multiple times', async () => {
  const projects = preparePackages([
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
    ...await filterPackagesFromDir(process.cwd(), [
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
})

test('recursive install --no-bail', async () => {
  const projects = preparePackages([
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
      ...await filterPackagesFromDir(process.cwd(), []),
      bail: false,
      dir: process.cwd(),
      recursive: true,
      workspaceDir: process.cwd(),
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_RECURSIVE_FAIL')

  expect(projects['project-2'].requireModule('is-negative')).toBeTruthy()
})

test('installing with "workspace=true" should work even if link-workspace-packages is off and save-workspace-protocol is false', async () => {
  const projects = preparePackages([
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
    ...await filterPackagesFromDir(process.cwd(), []),
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

  projects['project-1'].has('project-2')
})

test('installing with "workspace=true" should work even if link-workspace-packages is off and save-workspace-protocol is "rolling"', async () => {
  const projects = preparePackages([
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
    ...await filterPackagesFromDir(process.cwd(), []),
    dir: process.cwd(),
    linkWorkspacePackages: false,
    lockfileDir: process.cwd(),
    recursive: true,
    saveWorkspaceProtocol: 'rolling',
    sharedWorkspaceLockfile: true,
    workspace: true,
    workspaceDir: process.cwd(),
  }, ['project-2'])

  {
    const pkg = await import(path.resolve('project-1/package.json'))
    expect(pkg?.dependencies).toStrictEqual({ 'project-2': 'workspace:*' })
  }
  {
    const pkg = await import(path.resolve('project-2/package.json'))
    expect(pkg.dependencies).toBeFalsy()
  }

  projects['project-1'].has('project-2')
})

test('recursive install on workspace with custom lockfile-dir', async () => {
  preparePackages([
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
  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    lockfileDir,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  const lockfile = readYamlFile<LockfileFile>(path.join(lockfileDir, 'pnpm-lock.yaml'))
  expect(Object.keys(lockfile.importers!)).toStrictEqual(['../project-1', '../project-2'])
})

test('recursive install in a monorepo with different modules directories', async () => {
  const projects = preparePackages([
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
  fs.writeFileSync('project-1/.npmrc', 'modules-dir=modules_1', 'utf8')
  fs.writeFileSync('project-2/.npmrc', 'modules-dir=modules_2', 'utf8')

  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  projects['project-1'].has('is-positive', 'modules_1')
  projects['project-2'].has('is-positive', 'modules_2')
})

test('recursive install in a monorepo with parsing env variables', async () => {
  const projects = preparePackages([
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  process.env['SOME_NAME'] = 'some_name'
  // eslint-disable-next-line no-template-curly-in-string
  fs.writeFileSync('project/.npmrc', 'modules-dir=${SOME_NAME}_modules', 'utf8')

  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  projects['project'].has('is-positive', `${process.env['SOME_NAME']}_modules`)
})

test('prefer-workspace-package', async () => {
  await addDistTag({
    distTag: 'latest',
    package: '@pnpm.e2e/foo',
    version: '100.1.0',
  })
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/foo': '^100.0.0',
      },
    },
    {
      location: 'foo',
      package: {
        name: '@pnpm.e2e/foo',
        version: '100.0.0',
      },
    },
  ])

  await install.handler({
    ...DEFAULT_OPTS,
    ...await filterPackagesFromDir(process.cwd(), []),
    dir: process.cwd(),
    linkWorkspacePackages: true,
    preferWorkspacePackages: true,
    lockfileDir: process.cwd(),
    recursive: true,
    sharedWorkspaceLockfile: true,
    workspace: true,
    workspaceDir: process.cwd(),
  })

  const lockfile = readYamlFile<LockfileFile>(path.resolve('pnpm-lock.yaml'))
  expect(lockfile.importers?.['project-1'].dependencies?.['@pnpm.e2e/foo'].version).toBe('link:../foo')
})

test('installing in monorepo with shared lockfile should work on virtual drives', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])
  const virtualPath = process.cwd() + '-virtual-disk'
  // symlink simulates windows' subst
  await symlinkDir(process.cwd(), virtualPath)
  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterPackagesFromDir(virtualPath, [])
  await install.handler({
    ...DEFAULT_OPTS,
    lockfileDir: virtualPath,
    allProjects,
    allProjectsGraph,
    dir: virtualPath,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: virtualPath,
  })

  projects['project-1'].has('is-positive')
})

test('pass readPackage with shared lockfile', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
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
    ...await filterPackagesFromDir(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
    hooks: {
      readPackage: [
        (pkg) => ({
          ...pkg,
          dependencies: {
            'is-positive': '1.0.0',
          },
        }),
      ],
    },
  })

  projects['project-1'].has('is-positive')
  projects['project-1'].hasNot('is-negative')
  projects['project-2'].has('is-positive')
  projects['project-2'].hasNot('is-negative')
})
