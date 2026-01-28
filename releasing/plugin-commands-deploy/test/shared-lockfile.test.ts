import fs from 'fs'
import path from 'path'
import url from 'url'
import { install } from '@pnpm/plugin-commands-installation'
import { assertProject } from '@pnpm/assert-project'
import { preparePackages } from '@pnpm/prepare'
import { type PatchFile, type LockfileFile, type LockfilePackageSnapshot } from '@pnpm/lockfile.types'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { fixtures } from '@pnpm/test-fixtures'
import { type ProjectManifest } from '@pnpm/types'
import { jest } from '@jest/globals'
import writeYamlFile from 'write-yaml-file'
import { DEFAULT_OPTS } from './utils/index.js'

const f = fixtures(import.meta.dirname)

const resolvePathAsUrl = (...paths: string[]): string => url.pathToFileURL(path.resolve(...paths)).toString()

const original = await import('@pnpm/logger')
const warn = jest.fn()
jest.unstable_mockModule('@pnpm/logger', () => {
  const logger = {
    ...original.logger,
    warn,
  }
  return {
    ...original,
    globalWarn: jest.fn(),
    logger: Object.assign(() => logger, logger),
  }
})
const { globalWarn } = await import('@pnpm/logger')
const { deploy } = await import('@pnpm/plugin-commands-deploy')

beforeEach(async () => {
  jest.mocked(globalWarn).mockClear()
})

afterEach(() => {
  jest.restoreAllMocks()
})

function readPackageJson (manifestDir: string): unknown {
  const manifestPath = path.resolve(manifestDir, 'package.json')
  const manifestText = fs.readFileSync(manifestPath, 'utf-8')
  return JSON.parse(manifestText)
}

test('deploy with a shared lockfile after full install', async () => {
  const projectNames = ['project-1', 'project-2', 'project-3', 'project-4', 'project-5'] as const

  const preparedManifests: Record<typeof projectNames[number], ProjectManifest> = {
    'project-1': {
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
    'project-2': {
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
    'project-3': {
      name: 'project-3',
      version: '2.0.0',
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'project-5': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
    'project-4': {
      name: 'project-4',
      version: '0.0.0',
    },
    'project-5': {
      name: 'project-5',
      version: '0.0.0',
    },
  }

  preparePackages(projectNames.map(name => preparedManifests[name]))

  for (const name of projectNames) {
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
      'project-2': expect.stringMatching(/^project-2@file:/),
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'project-3': expect.stringMatching(/^project-3@file:/),
      'is-negative': '1.0.0',
    },
    files: ['index.js'],
    optionalDependencies: {},
    pnpm: {},
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
    expect(fs.existsSync('deploy/pnpm-lock.yaml')).toBeTruthy()
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
    expect(readPackageJson(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-2`)).toStrictEqual(preparedManifests['project-2'])
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
    expect(readPackageJson(`deploy/node_modules/.pnpm/${project3Name}/node_modules/project-3`)).toStrictEqual(preparedManifests['project-3'])
    const project4Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.startsWith('project-4@'))
    expect(project4Name).toBeDefined()
    expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project2Name}/node_modules/project-4`)).toBe(
      path.resolve(`deploy/node_modules/.pnpm/${project4Name}/node_modules/project-4`)
    )
    expect(readPackageJson(`deploy/node_modules/.pnpm/${project4Name}/node_modules/project-4`)).toStrictEqual(preparedManifests['project-4'])
    const project5Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.startsWith('project-5@'))
    expect(project5Name).toBeDefined()
    expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project3Name}/node_modules/project-5`)).toBe(
      path.resolve(`deploy/node_modules/.pnpm/${project5Name}/node_modules/project-5`)
    )
    expect(readPackageJson(`deploy/node_modules/.pnpm/${project5Name}/node_modules/project-5`)).toStrictEqual(preparedManifests['project-5'])
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
    expect(fs.existsSync('deploy/pnpm-lock.yaml')).toBeTruthy()
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

test('the deploy manifest should inherit some fields from the workspace manifest', async () => {
  const workspaceSettings = {
    allowBuilds: { 'from-root': true },
    overrides: {
      'is-positive': '2.0.0',
    },
  }

  const preparedManifests: Record<'root' | 'project-0', ProjectManifest> = {
    root: {
      name: 'root',
      version: '0.0.0',
      private: true,
    },
    'project-0': {
      name: 'project-0',
      version: '0.0.0',
      private: true,
      dependencies: {
        'is-positive': '3.1.0',
      },
    },
  }

  preparePackages([
    {
      location: '.',
      package: preparedManifests.root,
    },
    preparedManifests['project-0'],
  ])

  writeYamlFile.sync('pnpm-workspace.yaml', {
    packages: ['project-0'],
    ...workspaceSettings,
  })

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
    overrides: workspaceSettings.overrides,
    allowBuilds: workspaceSettings.allowBuilds,
    recursive: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  })
  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    rootProjectManifest: {
      ...preparedManifests.root,
      pnpm: { allowBuilds: workspaceSettings.allowBuilds },
    },
    rootProjectManifestDir: process.cwd(),
    allowBuilds: workspaceSettings.allowBuilds,
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }, ['deploy'])

  const project = assertProject(path.resolve('deploy'))
  project.has('is-positive')

  const manifest = readPackageJson('deploy') as ProjectManifest
  expect(manifest.pnpm).toStrictEqual({
    allowBuilds: workspaceSettings.allowBuilds,
  } as ProjectManifest['pnpm'])

  expect(readPackageJson('deploy/node_modules/is-positive/')).toHaveProperty(['version'], workspaceSettings.overrides['is-positive'])
  expect(project.readLockfile().importers).toStrictEqual({
    '.': {
      dependencies: {
        'is-positive': {
          specifier: workspaceSettings.overrides['is-positive'],
          version: workspaceSettings.overrides['is-positive'],
        },
      },
    },
  } as LockfileFile['importers'])
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
          version: expect.stringMatching(/^prod-1@file:/),
          specifier: expect.stringMatching(/^prod-1@file:/),
        },
      },
      devDependencies: {
        'dev-0': {
          version: expect.stringMatching(/^dev-0@file:/),
          specifier: expect.stringMatching(/^dev-0@file:/),
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
      'prod-1': expect.stringMatching(/^prod-1@file:/),
    },
    devDependencies: {
      'dev-0': expect.stringMatching(/^dev-0@file:/),
      'is-negative': '1.0.0',
    },
    optionalDependencies: {},
    pnpm: {},
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
          version: expect.stringMatching(/^project-1@file:/),
          specifier: expect.stringMatching(/^project-1@file:/),
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
      'project-1': expect.stringMatching(/^project-1@file:/),
    },
    devDependencies: {},
    optionalDependencies: {},
    pnpm: {},
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
    pnpm: {},
  } as ProjectManifest)

  expect(fs.realpathSync('deploy/node_modules/project-0')).toBe(path.resolve('deploy'))
  expect(fs.realpathSync('deploy/node_modules/renamed-workspace')).toBe(path.resolve('deploy'))
  expect(fs.realpathSync('deploy/node_modules/renamed-linked')).toBe(path.resolve('deploy'))
})

test('deploy with a shared lockfile should correctly handle packageExtensions', async () => {
  const packageExtensions = {
    'is-positive': {
      dependencies: {
        'is-odd': '1.0.0',
        'link-to-project-0': 'link:project-0',
        'link-to-project-1': 'link:project-1',
        'project-0': 'workspace:*',
        'project-1': 'workspace:*',
      },
    },
  }

  const preparedManifests: Record<string, ProjectManifest> = {
    root: {
      name: 'root',
      version: '0.0.0',
      private: true,
    },
    'project-0': {
      name: 'project-0',
      version: '0.0.0',
      dependencies: {
        'project-1': 'workspace:*',
      },
    },
    'project-1': {
      name: 'project-1',
      version: '0.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  }

  preparePackages([
    {
      location: '.',
      package: preparedManifests.root,
    },
    preparedManifests['project-0'],
    preparedManifests['project-1'],
  ])

  writeYamlFile.sync('pnpm-workspace.yaml', {
    packages: ['project-0', 'project-1'],
    packageExtensions,
  })

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
    rootProjectManifest: {
      ...preparedManifests.root,
      pnpm: { packageExtensions },
    },
    rootProjectManifestDir: process.cwd(),
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
  expect(lockfile).toHaveProperty(['snapshots', 'is-positive@1.0.0'], {
    dependencies: {
      'is-odd': '1.0.0',
      'link-to-project-0': 'link:.',
      'link-to-project-1': expect.stringMatching(/^project-1@file:/),
      'project-0': 'link:.',
      'project-1': expect.stringMatching(/^project-1@file:/),
    },
  } as LockfilePackageSnapshot)

  const manifest = readPackageJson('deploy') as ProjectManifest
  expect(manifest).toStrictEqual({
    name: 'project-0',
    version: '0.0.0',
    dependencies: {
      'project-1': expect.stringMatching(/^project-1@file:/),
    },
    devDependencies: {},
    optionalDependencies: {},
    pnpm: {},
  } as ProjectManifest)

  const project1Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.includes('project-1@'))
  expect(project1Name).toBeDefined()

  expect(fs.realpathSync('deploy/node_modules/.pnpm/is-positive@1.0.0/node_modules/is-odd'))
    .toBe(path.resolve('deploy/node_modules/.pnpm/is-odd@1.0.0/node_modules/is-odd'))
  expect(fs.realpathSync('deploy/node_modules/.pnpm/is-positive@1.0.0/node_modules/link-to-project-0')).toBe(path.resolve('deploy'))
  expect(fs.realpathSync('deploy/node_modules/.pnpm/is-positive@1.0.0/node_modules/link-to-project-1'))
    .toBe(path.resolve(`deploy/node_modules/.pnpm/${project1Name}/node_modules/project-1`))
  expect(fs.realpathSync('deploy/node_modules/.pnpm/is-positive@1.0.0/node_modules/project-0')).toBe(path.resolve('deploy'))
  expect(fs.realpathSync('deploy/node_modules/.pnpm/is-positive@1.0.0/node_modules/project-1'))
    .toBe(path.resolve(`deploy/node_modules/.pnpm/${project1Name}/node_modules/project-1`))

  expect(readPackageJson('deploy/node_modules/.pnpm/is-positive@1.0.0/node_modules/link-to-project-0')).toStrictEqual(manifest)
  expect(readPackageJson('deploy/node_modules/.pnpm/is-positive@1.0.0/node_modules/link-to-project-1')).toStrictEqual(preparedManifests['project-1'])
  expect(readPackageJson('deploy/node_modules/.pnpm/is-positive@1.0.0/node_modules/project-0')).toStrictEqual(manifest)
  expect(readPackageJson('deploy/node_modules/.pnpm/is-positive@1.0.0/node_modules/project-1')).toStrictEqual(preparedManifests['project-1'])
})

test('deploy with a shared lockfile should correctly handle patchedDependencies', async () => {
  const patchedDependencies = {
    'is-positive': '__patches__/is-positive.patch',
  }
  const preparedManifests: Record<string, ProjectManifest> = {
    root: {
      name: 'root',
      version: '0.0.0',
      private: true,
    },
    'project-0': {
      name: 'project-0',
      version: '0.0.0',
      dependencies: {
        'project-1': 'workspace:*',
      },
    },
    'project-1': {
      name: 'project-1',
      version: '0.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  }

  preparePackages([
    {
      location: '.',
      package: preparedManifests.root,
    },
    preparedManifests['project-0'],
    preparedManifests['project-1'],
  ])

  writeYamlFile.sync('pnpm-workspace.yaml', {
    packages: ['project-0', 'project-1'],
    patchedDependencies,
  })

  f.copy('is-positive.patch', '__patches__/is-positive.patch')

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
    patchedDependencies,
    recursive: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  })
  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    patchedDependencies,
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }, ['deploy'])

  const project = assertProject(path.resolve('deploy'))
  project.has('project-1')

  const lockfile = project.readLockfile()
  expect(lockfile.patchedDependencies).toStrictEqual({
    'is-positive': {
      hash: expect.any(String),
      path: '../__patches__/is-positive.patch',
    },
  } as Record<string, PatchFile>)

  const patchFile = lockfile.patchedDependencies['is-positive']

  const manifest = readPackageJson('deploy') as ProjectManifest
  expect(manifest).toStrictEqual({
    name: 'project-0',
    version: '0.0.0',
    dependencies: {
      'project-1': expect.stringMatching(/^project-1@file:/),
    },
    devDependencies: {},
    optionalDependencies: {},
    pnpm: {
      patchedDependencies: {
        'is-positive': '../__patches__/is-positive.patch',
      },
    },
  } as ProjectManifest)

  const project1Name = fs.readdirSync('deploy/node_modules/.pnpm').find(name => name.includes('project-1@'))
  expect(project1Name).toBeDefined()
  if (process.platform !== 'win32') {
    expect(fs.realpathSync(`deploy/node_modules/.pnpm/${project1Name}/node_modules/is-positive`)).toBe(
      path.resolve(`deploy/node_modules/.pnpm/is-positive@1.0.0_patch_hash=${patchFile.hash}/node_modules/is-positive`)
    )
  }
  expect(
    fs.readFileSync(`deploy/node_modules/.pnpm/${project1Name}/node_modules/is-positive/PATCH.txt`, 'utf-8')
      .trim()
  ).toBe('added by pnpm patch-commit')
})

test('deploy with a shared lockfile that has peer dependencies suffix in workspace package dependency paths', async () => {
  const preparedManifests: Record<string, ProjectManifest> = {
    'project-0': {
      name: 'project-0',
      version: '0.0.0',
      dependencies: {
        'project-1': 'workspace:*',
      },
      peerDependencies: {
        'project-1': '*',
        'project-2': '*',
      },
    },
    'project-1': {
      name: 'project-1',
      version: '0.0.0',
      dependencies: {
        'is-positive': '1.0.0',
        'project-2': 'workspace:*',
      },
      peerDependencies: {
        'is-negative': '>=1.0.0',
        'project-2': '*',
      },
    },
    'project-2': {
      name: 'project-2',
      version: '0.0.0',
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
    },
  }

  preparePackages(['project-0', 'project-1', 'project-2'].map(name => ({
    location: `packages/${name}`,
    package: preparedManifests[name],
  })))

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
    dedupeInjectedDeps: false,
    dir: process.cwd(),
    recursive: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  })
  expect(assertProject('.').readLockfile()).toMatchObject({
    importers: {
      'packages/project-0': {
        dependencies: {
          'project-1': {
            version: 'file:packages/project-1(is-negative@1.0.0)(project-2@file:packages/project-2(is-positive@1.0.0))',
          },
          'project-2': {
            version: 'file:packages/project-2(is-positive@1.0.0)',
          },
        },
      },
      'packages/project-1': {
        dependencies: {
          'project-2': {
            version: 'file:packages/project-2(is-positive@1.0.0)',
          },
        },
      },
    },
    packages: {
      'project-1@file:packages/project-1': {
        resolution: {
          type: 'directory',
          directory: 'packages/project-1',
        },
      },
      'project-2@file:packages/project-2': {
        resolution: {
          type: 'directory',
          directory: 'packages/project-2',
        },
      },
    },
    snapshots: {
      'project-1@file:packages/project-1(is-negative@1.0.0)(project-2@file:packages/project-2(is-positive@1.0.0))': {
        dependencies: {
          'project-2': 'file:packages/project-2(is-positive@1.0.0)',
        },
      },
      'project-2@file:packages/project-2(is-positive@1.0.0)': {},
    },
  })

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
  project.has('project-2')

  expect(project.readLockfile()).toMatchObject({
    importers: {
      '.': {
        dependencies: {
          'project-1': {
            specifier: `project-1@${resolvePathAsUrl('packages/project-1')}(is-negative@1.0.0)(project-2@file:packages/project-2(is-positive@1.0.0))`,
            version: `project-1@${resolvePathAsUrl('packages/project-1')}(is-negative@1.0.0)(project-2@file:packages/project-2(is-positive@1.0.0))`,
          },
          'project-2': {
            specifier: `project-2@${resolvePathAsUrl('packages/project-2')}(is-positive@1.0.0)`,
            version: `project-2@${resolvePathAsUrl('packages/project-2')}(is-positive@1.0.0)`,
          },
        },
      },
    },
    packages: {
      [`project-1@${resolvePathAsUrl('packages/project-1')}`]: {
        resolution: {
          type: 'directory',
          directory: '../packages/project-1',
        },
      },
      [`project-2@${resolvePathAsUrl('packages/project-2')}`]: {
        resolution: {
          type: 'directory',
          directory: '../packages/project-2',
        },
      },
    },
    snapshots: {
      [`project-1@${resolvePathAsUrl('packages/project-1')}(is-negative@1.0.0)(project-2@file:packages/project-2(is-positive@1.0.0))`]: {
        dependencies: {
          'project-2': `project-2@${resolvePathAsUrl('packages/project-2')}(is-positive@1.0.0)`,
        },
      },
      [`project-2@${resolvePathAsUrl('packages/project-2')}(is-positive@1.0.0)`]: {},
    },
  })

  expect(readPackageJson('deploy')).toStrictEqual({
    name: 'project-0',
    version: '0.0.0',
    dependencies: {
      'project-1': `project-1@${resolvePathAsUrl('packages/project-1')}(is-negative@1.0.0)(project-2@file:packages/project-2(is-positive@1.0.0))`,
      'project-2': `project-2@${resolvePathAsUrl('packages/project-2')}(is-positive@1.0.0)`,
    },
    devDependencies: {},
    optionalDependencies: {},
    peerDependencies: {
      'project-1': '*',
      'project-2': '*',
    },
    pnpm: {},
  } as ProjectManifest)

  expect(readPackageJson('deploy/node_modules/project-1')).toStrictEqual(preparedManifests['project-1'])
  expect(readPackageJson('deploy/node_modules/project-2')).toStrictEqual(preparedManifests['project-2'])

  const project1Names = fs.readdirSync('deploy/node_modules/.pnpm').filter(name => name.includes('project-1@'))
  expect(project1Names).not.toStrictEqual([])
  for (const name of project1Names) {
    expect(readPackageJson(`deploy/node_modules/.pnpm/${name}/node_modules/project-1`)).toStrictEqual(preparedManifests['project-1'])
  }

  const project2Names = fs.readdirSync('deploy/node_modules/.pnpm').filter(name => name.includes('project-2@'))
  expect(project2Names).not.toStrictEqual([])
  for (const name of project2Names) {
    expect(readPackageJson(`deploy/node_modules/.pnpm/${name}/node_modules/project-2`)).toStrictEqual(preparedManifests['project-2'])
  }
})

test('deploy with a shared lockfile should keep files created by lifecycle scripts', async () => {
  const preparedManifests: Record<string, ProjectManifest> = {
    root: {
      name: 'root',
      version: '0.0.0',
      private: true,
    },
    'project-0': {
      name: 'project-0',
      version: '0.0.0',
      dependencies: {
        '@pnpm.e2e/install-script-example': '*',
      },
    },
  }

  preparePackages([
    {
      location: '.',
      package: preparedManifests.root,
    },
    preparedManifests['project-0'],
  ])
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['project-0', '!store/**'] })

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
    rootProjectManifest: preparedManifests.root,
    rootProjectManifestDir: process.cwd(),
    recursive: true,
    lockfileDir: process.cwd(),
    allowBuilds: { '@pnpm.e2e/install-script-example': true },
    workspaceDir: process.cwd(),
  })
  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()
  expect(fs.existsSync('project-0/node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    rootProjectManifest: preparedManifests.root,
    rootProjectManifestDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    allowBuilds: { '@pnpm.e2e/install-script-example': true },
    workspaceDir: process.cwd(),
  }, ['deploy'])

  expect(fs.existsSync('deploy/node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})
