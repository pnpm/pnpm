import fs from 'fs'
import path from 'path'
import { deploy } from '@pnpm/plugin-commands-deploy'
import { install } from '@pnpm/plugin-commands-installation'
import { assertProject } from '@pnpm/assert-project'
import { preparePackages } from '@pnpm/prepare'
import { type LockfileFile } from '@pnpm/lockfile.types'
import { logger, globalWarn } from '@pnpm/logger'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { type ProjectManifest } from '@pnpm/types'
import { DEFAULT_OPTS } from './utils'

beforeEach(async () => {
  const logger = await import('@pnpm/logger')
  jest.spyOn(logger, 'globalWarn')
})

afterEach(() => {
  jest.restoreAllMocks()
})

function readPackageJson (manifestDir: string): unknown {
  const manifestPath = path.resolve(manifestDir, 'package.json')
  const manifestText = fs.readFileSync(manifestPath, 'utf-8')
  return JSON.parse(manifestText)
}

test('deploy without existing lockfile', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      files: ['index.js'],
      dependencies: {
        'project-2': 'workspace:*',
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'project-3': 'workspace:*',
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '2.0.0',
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
  ])

  for (const name of ['project-1', 'project-2', 'project-3']) {
    fs.writeFileSync(`${name}/test.js`, '', 'utf8')
    fs.writeFileSync(`${name}/index.js`, '', 'utf8')
  }

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-1' }])

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    dev: false,
    production: true,
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }, ['deploy'])

  expect(globalWarn).toHaveBeenCalledWith('Shared lockfile not found. Falling back to installing without a lockfile.')

  const project = assertProject(path.resolve('deploy'))
  project.has('project-2')
  project.has('is-positive')
  project.hasNot('project-3')
  project.hasNot('is-negative')
  expect(fs.existsSync('deploy/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/test.js')).toBeFalsy()
  expect(fs.existsSync('deploy/node_modules/.modules.yaml')).toBeTruthy()
  expect(fs.existsSync('deploy/node_modules/.pnpm/project-2@file+project-2/node_modules/project-2/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/node_modules/.pnpm/project-2@file+project-2/node_modules/project-2/test.js')).toBeFalsy()
  expect(fs.existsSync('deploy/node_modules/.pnpm/project-3@file+project-3/node_modules/project-3/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/node_modules/.pnpm/project-3@file+project-3/node_modules/project-3/test.js')).toBeFalsy()
  expect(fs.existsSync('pnpm-lock.yaml')).toBeFalsy() // no changes to the lockfile are written
})

test('deploy with a shared lockfile after full install', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      files: ['index.js'],
      dependencies: {
        'project-2': 'workspace:*',
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'project-3': 'workspace:*',
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'project-4': 'workspace:*',
        'renamed-project-2': 'workspace:project-2@*',
        'is-odd': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '2.0.0',
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'project-5': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
    {
      name: 'project-4',
      version: '0.0.0',
    },
    {
      name: 'project-5',
      version: '0.0.0',
    },
  ])

  for (const name of ['project-1', 'project-2', 'project-3', 'project-4', 'project-5']) {
    fs.writeFileSync(`${name}/test.js`, '', 'utf8')
    fs.writeFileSync(`${name}/index.js`, '', 'utf8')
  }

  const {
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-1' }])

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph: allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  })
  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()

  const expectedDeployManifest: ProjectManifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'project-2': expect.stringContaining('file:'),
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'project-3': expect.stringContaining('file:'),
      'is-negative': '1.0.0',
    },
    optionalDependencies: {},
  }

  // deploy prod only
  {
    fs.rmSync('deploy', { recursive: true, force: true })
    await deploy.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      dev: false,
      production: true,
      recursive: true,
      selectedProjectsGraph,
      sharedWorkspaceLockfile: true,
      lockfileDir: process.cwd(),
      workspaceDir: process.cwd(),
    }, ['deploy'])

    const project = assertProject(path.resolve('deploy'))
    project.has('project-2')
    project.has('is-positive')
    project.hasNot('project-3')
    project.hasNot('is-negative')
    project.hasNot('project-4')
    project.hasNot('project-5')
    expect(readPackageJson('deploy')).toStrictEqual(expectedDeployManifest)
    expect(fs.existsSync('deploy/pnpm-lock.yaml'))
    expect(fs.existsSync('deploy/index.js')).toBeTruthy()
    expect(fs.existsSync('deploy/test.js')).toBeFalsy()
    expect(fs.existsSync('deploy/node_modules/.modules.yaml')).toBeTruthy()
    const project2Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.startsWith('project-2@'))
    expect(project2Name).toBeDefined()
    expect(fs.realpathSync('deploy/node_modules/project-2')).toBe(path.resolve(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-2`))
    expect(fs.existsSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-2/index.js`)).toBeTruthy()
    expect(fs.existsSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-2/test.js`)).toBeFalsy()
    expect(fs.readdirSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules`).sort()).toStrictEqual([
      'is-odd',
      'project-2',
      'project-3',
      'project-4',
      'renamed-project-2',
    ])
    expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules/renamed-project-2`)).toBe(
      path.resolve(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-2`)
    )
    const project3Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.startsWith('project-3@'))
    expect(project3Name).toBeDefined()
    expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-3`)).toBe(
      path.resolve(`deploy/node_modules/.pnpm/${project3Name}/node_modules/project-3`)
    )
    expect(fs.readdirSync(`deploy/node_modules/.pnpm/${project3Name}/node_modules`).sort()).toStrictEqual([
      'is-odd',
      'project-3',
      'project-5',
    ])
    const project4Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.startsWith('project-4@'))
    expect(project4Name).toBeDefined()
    expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-4`)).toBe(
      path.resolve(`deploy/node_modules/.pnpm/${project4Name}/node_modules/project-4`)
    )
    const project5Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.startsWith('project-5@'))
    expect(project5Name).toBeDefined()
    expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project3Name}/node_modules/project-5`)).toBe(
      path.resolve(`deploy/node_modules/.pnpm/${project5Name}/node_modules/project-5`)
    )
    expect(globalWarn).not.toHaveBeenCalledWith(expect.stringContaining('Falling back to installing without a lockfile'))
  }

  // deploy all
  {
    fs.rmSync('deploy', { recursive: true, force: true })
    await deploy.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      sharedWorkspaceLockfile: true,
      lockfileDir: process.cwd(),
      workspaceDir: process.cwd(),
    }, ['deploy'])

    const project = assertProject(path.resolve('deploy'))
    project.has('project-2')
    project.has('is-positive')
    project.has('project-3')
    project.has('is-negative')
    project.hasNot('project-4')
    project.hasNot('project-5')
    expect(readPackageJson('deploy')).toStrictEqual(expectedDeployManifest)
    expect(fs.existsSync('deploy/pnpm-lock.yaml'))
    expect(fs.existsSync('deploy/index.js')).toBeTruthy()
    expect(fs.existsSync('deploy/test.js')).toBeFalsy()
    expect(fs.existsSync('deploy/node_modules/.modules.yaml')).toBeTruthy()
    const project2Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.startsWith('project-2@'))
    expect(project2Name).toBeDefined()
    expect(fs.realpathSync('deploy/node_modules/project-2')).toBe(path.resolve(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-2`))
    expect(fs.existsSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-2/index.js`)).toBeTruthy()
    expect(fs.existsSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-2/test.js`)).toBeFalsy()
    expect(fs.readdirSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules`).sort()).toStrictEqual([
      'is-odd',
      'project-2',
      'project-3',
      'project-4',
      'renamed-project-2',
    ])
    const project3Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.startsWith('project-3@'))
    expect(project3Name).toBeDefined()
    expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-3`)).toBe(
      path.resolve(`deploy/node_modules/.pnpm/${project3Name}/node_modules/project-3`)
    )
    expect(project3Name).toBeDefined()
    expect(fs.existsSync(`deploy/node_modules/.pnpm/${project3Name}/node_modules/project-3/index.js`)).toBeTruthy()
    expect(fs.existsSync(`deploy/node_modules/.pnpm/${project3Name}/node_modules/project-3/test.js`)).toBeFalsy()
    expect(fs.readdirSync(`deploy/node_modules/.pnpm/${project3Name}/node_modules`).sort()).toStrictEqual([
      'is-odd',
      'project-3',
      'project-5',
    ])
    expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project3Name}/node_modules/project-3`)).toContain(project3Name)
    const project4Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.startsWith('project-4@'))
    expect(project4Name).toBeDefined()
    expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-4`)).toBe(
      path.resolve(`deploy/node_modules/.pnpm/${project4Name}/node_modules/project-4`)
    )
    const project5Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.startsWith('project-5@'))
    expect(project5Name).toBeDefined()
    expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project3Name}/node_modules/project-5`)).toBe(
      path.resolve(`deploy/node_modules/.pnpm/${project5Name}/node_modules/project-5`)
    )
    expect(globalWarn).not.toHaveBeenCalledWith(expect.stringContaining('Falling back to installing without a lockfile'))
  }
})

test('deploy with a shared lockfile and --prod filter should not fail even if dev workspace package does not exist (#8778)', async () => {
  preparePackages([
    {
      name: 'prod-0',
      version: '0.0.0',
      private: true,
      dependencies: {
        'prod-1': 'workspace:*',
      },
      devDependencies: {
        'dev-0': 'workspace:*',
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'prod-1',
      version: '0.0.0',
      private: true,
      dependencies: {
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'dev-1': 'workspace:*',
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'dev-0',
      version: '0.0.0',
      private: true,
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'dev-1',
      version: '0.0.0',
      private: true,
    },
  ])

  const {
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'prod-0' }])

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph: allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  })
  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()

  fs.rmSync('dev-0', { recursive: true })
  fs.rmSync('dev-1', { recursive: true })

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    production: true,
    dev: false,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }, ['deploy'])

  const project = assertProject(path.resolve('deploy'))
  project.has('prod-1')
  project.hasNot('dev-0')
  project.hasNot('dev-1')

  const lockfile = project.readLockfile()
  expect(lockfile.importers).toStrictEqual({
    '.': {
      dependencies: {
        'prod-1': {
          version: expect.stringContaining('prod-1'),
          specifier: expect.stringContaining('file:'),
        },
      },
      devDependencies: {
        'dev-0': {
          version: expect.stringContaining('dev-0'),
          specifier: expect.stringContaining('file:'),
        },
        'is-negative': {
          version: '1.0.0',
          specifier: '1.0.0',
        },
      },
    },
  } as LockfileFile['importers'])

  const manifest = readPackageJson('deploy') as ProjectManifest
  expect(manifest).toStrictEqual({
    name: 'prod-0',
    version: '0.0.0',
    private: true,
    dependencies: {
      'prod-1': expect.stringContaining('prod-1'),
    },
    devDependencies: {
      'dev-0': expect.stringContaining('dev-0'),
      'is-negative': '1.0.0',
    },
    optionalDependencies: {},
  } as ProjectManifest)

  const prod1Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.includes('prod-1@'))
  expect(prod1Name).toBeDefined()
  expect(fs.readdirSync(`deploy/node_modules/.pnpm/${prod1Name}/node_modules`).sort()).toStrictEqual(['is-positive', 'prod-1'])
  expect(fs.realpathSync('deploy/node_modules/prod-1')).toBe(path.resolve(`deploy/node_modules/.pnpm/${prod1Name}/node_modules/prod-1`))
})

test('deploy with a shared lockfile should correctly handle workspace dependencies that depend on the deployed project', async () => {
  preparePackages([
    {
      name: 'project-0',
      version: '0.0.0',
      private: true,
      dependencies: {
        'project-1': 'workspace:*',
      },
    },
    {
      name: 'project-1',
      version: '0.0.0',
      private: true,
      dependencies: {
        'project-0': 'workspace:*',
      },
    },
  ])

  const {
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-0' }])

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph: allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  })
  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }, ['deploy'])

  const project = assertProject(path.resolve('deploy'))
  project.has('project-1')

  const lockfile = project.readLockfile()
  expect(lockfile.importers).toStrictEqual({
    '.': {
      dependencies: {
        'project-1': {
          version: expect.stringContaining('project-1'),
          specifier: expect.stringContaining('file:'),
        },
      },
    },
  } as LockfileFile['importers'])

  const manifest = readPackageJson('deploy') as ProjectManifest
  expect(manifest).toStrictEqual({
    name: 'project-0',
    version: '0.0.0',
    private: true,
    dependencies: {
      'project-1': expect.stringContaining('project-1'),
    },
    devDependencies: {},
    optionalDependencies: {},
  } as ProjectManifest)

  const project1Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.includes('project-1@'))
  expect(project1Name).toBeDefined()
  expect(fs.readdirSync(`deploy/node_modules/.pnpm/${project1Name}/node_modules`).sort()).toStrictEqual(['project-0', 'project-1'])
  expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project1Name}/node_modules/project-0`)).toBe(path.resolve('deploy'))
  expect(fs.realpathSync('deploy/node_modules/project-1')).toBe(path.resolve(`deploy/node_modules/.pnpm/${project1Name}/node_modules/project-1`))
})

test('deploy with a shared lockfile should correctly handle package that depends on itself', async () => {
  preparePackages([
    {
      name: 'project-0',
      version: '0.0.0',
      private: true,
      dependencies: {
        'project-0': 'workspace:*',
        'renamed-workspace': 'workspace:project-0@*',
        'renamed-linked': 'link:.',
      },
    },
  ])

  const {
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-0' }])

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph: allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  })
  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }, ['deploy'])

  const project = assertProject(path.resolve('deploy'))
  project.has('project-0')
  project.has('renamed-workspace')
  project.has('renamed-linked')

  const lockfile = project.readLockfile()
  expect(lockfile.importers).toStrictEqual({
    '.': {
      dependencies: {
        'project-0': {
          version: 'link:.',
          specifier: 'link:.',
        },
        'renamed-workspace': {
          version: 'link:.',
          specifier: 'link:.',
        },
        'renamed-linked': {
          version: 'link:.',
          specifier: 'link:.',
        },
      },
    },
  } as LockfileFile['importers'])

  const manifest = readPackageJson('deploy') as ProjectManifest
  expect(manifest).toStrictEqual({
    name: 'project-0',
    version: '0.0.0',
    private: true,
    dependencies: {
      'project-0': 'link:.',
      'renamed-workspace': 'link:.',
      'renamed-linked': 'link:.',
    },
    devDependencies: {},
    optionalDependencies: {},
  } as ProjectManifest)

  expect(fs.realpathSync('deploy/node_modules/project-0')).toBe(path.resolve('deploy'))
  expect(fs.realpathSync('deploy/node_modules/renamed-workspace')).toBe(path.resolve('deploy'))
  expect(fs.realpathSync('deploy/node_modules/renamed-linked')).toBe(path.resolve('deploy'))
})

test('deploy in workspace with shared-workspace-lockfile=false', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      files: ['index.js'],
      dependencies: {
        'project-2': 'workspace:*',
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'project-3': 'workspace:*',
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '2.0.0',
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
  ])

  for (const name of ['project-1', 'project-2', 'project-3']) {
    fs.writeFileSync(`${name}/test.js`, '', 'utf8')
    fs.writeFileSync(`${name}/index.js`, '', 'utf8')
  }

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-1' }])

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    dev: false,
    production: true,
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: false,
    workspaceDir: process.cwd(),
  }, ['deploy'])

  const project = assertProject(path.resolve('deploy'))
  project.has('project-2')
  project.has('is-positive')
  project.hasNot('project-3')
  project.hasNot('is-negative')
  expect(fs.existsSync('deploy/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/test.js')).toBeFalsy()
  expect(fs.existsSync('deploy/node_modules/.modules.yaml')).toBeTruthy()
  expect(fs.existsSync('deploy/node_modules/.pnpm/project-2@file+..+project-2/node_modules/project-2/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/node_modules/.pnpm/project-2@file+..+project-2/node_modules/project-2/test.js')).toBeFalsy()
  expect(fs.existsSync('deploy/node_modules/.pnpm/project-3@file+..+project-3/node_modules/project-3/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/node_modules/.pnpm/project-3@file+..+project-3/node_modules/project-3/test.js')).toBeFalsy()
  expect(fs.existsSync('pnpm-lock.yaml')).toBeFalsy() // no changes to the lockfile are written
})

test('deploy with node-linker=hoisted', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
      },
    },
    {
      name: 'project-1',
      version: '1.0.0',
      files: ['index.js'],
      dependencies: {
        'project-2': 'workspace:*',
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'project-3': 'workspace:*',
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '2.0.0',
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
  ])

  ; ['project-1', 'project-2', 'project-3'].forEach(name => {
    fs.writeFileSync(`${name}/test.js`, '', 'utf8')
    fs.writeFileSync(`${name}/index.js`, '', 'utf8')
  })

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-1' }])

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    dev: false,
    production: true,
    recursive: true,
    selectedProjectsGraph,
    nodeLinker: 'hoisted',
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }, ['dist'])

  const project = assertProject(path.resolve('dist'))
  project.has('project-2')
  project.has('is-positive')
  project.has('project-3')
  project.hasNot('is-negative')
  expect(fs.existsSync('dist/index.js')).toBeTruthy()
  expect(fs.existsSync('dist/test.js')).toBeFalsy()
  expect(fs.existsSync('dist/node_modules/.modules.yaml')).toBeTruthy()
  expect(fs.existsSync('dist/node_modules/project-2/index.js')).toBeTruthy()
  expect(fs.existsSync('dist/node_modules/project-2/test.js')).toBeFalsy()
  expect(fs.existsSync('dist/node_modules/project-3/index.js')).toBeTruthy()
  expect(fs.existsSync('dist/node_modules/project-3/test.js')).toBeFalsy()
  expect(fs.existsSync('pnpm-lock.yaml')).toBeFalsy() // no changes to the lockfile are written
})

test('deploy fails when the destination directory exists and is not empty', async () => {
  preparePackages([
    {
      name: 'project',
      version: '1.0.0',
      files: ['index.js'],
      dependencies: {},
      devDependencies: {},
    },
  ])
  fs.writeFileSync('project/index.js', '', 'utf8')

  const deployPath = 'deploy'
  fs.writeFileSync(deployPath, 'aaa', 'utf8')
  const deployFullPath = path.resolve(deployPath)

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project' }])

  await expect(() =>
    deploy.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      dev: false,
      production: true,
      recursive: true,
      selectedProjectsGraph,
      sharedWorkspaceLockfile: true,
      lockfileDir: process.cwd(),
      workspaceDir: process.cwd(),
    }, [deployPath])).rejects.toThrow(`Deploy path ${deployFullPath} is not empty`)

  expect(fs.existsSync(`${deployPath}/index.js`)).toBeFalsy() // no changes to the deploy path are made
  expect(fs.existsSync('pnpm-lock.yaml')).toBeFalsy() // no changes to the lockfile are written
})

test('forced deploy succeeds with a warning when destination directory exists and is not empty', async () => {
  const warnMock = jest.spyOn(logger, 'warn')

  preparePackages([
    {
      name: 'project',
      version: '1.0.0',
      files: ['index.js'],
      dependencies: {
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])
  fs.writeFileSync('project/index.js', '', 'utf8')

  const deployPath = 'deploy'
  fs.writeFileSync(deployPath, 'aaa', 'utf8')
  const deployFullPath = path.resolve(deployPath)

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project' }])

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    dev: false,
    production: true,
    recursive: true,
    force: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }, [deployPath])

  expect(warnMock).toHaveBeenCalledWith({
    message: expect.stringMatching(/^using --force, deleting deploy pat/),
    prefix: deployFullPath,
  })

  // deployed successfully
  const project = assertProject(deployFullPath)
  project.has('is-positive')
  project.hasNot('is-negative')
  expect(fs.existsSync('deploy/index.js')).toBeTruthy()
  expect(fs.existsSync('pnpm-lock.yaml')).toBeFalsy() // no changes to the lockfile are written

  warnMock.mockRestore()
})

test('deploy with dedupePeerDependents=true ignores the value of dedupePeerDependents', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      location: './sub-dir/project-2',
      package: {
        name: 'project-2',
        version: '2.0.0',
        dependencies: {
          'is-odd': '1.0.0',
        },
      },
    },
    {
      name: 'project-3',
      version: '2.0.0',
      dependencies: {
        'is-number': '1.0.0',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph, allProjectsGraph } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-1' }])

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    dev: false,
    production: true,
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
    dedupePeerDependents: true, // This is ignored by deploy
  }, ['deploy'])
  const project = assertProject(path.resolve('deploy'))
  project.has('is-positive')
  expect(fs.existsSync('sub-dir/deploy')).toBe(false)
})

// Regression test for https://github.com/pnpm/pnpm/issues/8297 (pnpm deploy doesn't replace catalog: protocol)
test('deploy works when workspace packages use catalog protocol', async () => {
  preparePackages([
    {
      name: 'project-1',
      dependencies: {
        'project-2': 'workspace:*',
        'is-positive': 'catalog:',
      },
    },
    {
      name: 'project-2',
      dependencies: {
        'project-3': 'workspace:*',
        'is-positive': 'catalog:',
      },
    },
    {
      name: 'project-3',
      dependencies: {
        'project-3': 'workspace:*',
        'is-positive': 'catalog:',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-1' }])

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    catalogs: {
      default: {
        'is-positive': '1.0.0',
      },
    },
    dir: process.cwd(),
    dev: false,
    production: true,
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }, ['deploy'])

  // Make sure the is-positive cataloged dependency was actually installed.
  expect(fs.existsSync('deploy/node_modules/.pnpm/project-3@file+project-3/node_modules/is-positive')).toBeTruthy()
})
