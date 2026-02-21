import fs from 'fs'
import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { preparePackages } from '@pnpm/prepare'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { jest } from '@jest/globals'
import { DEFAULT_OPTS } from './utils/index.js'
import { install } from '@pnpm/plugin-commands-installation'

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

test('deploy provides hint to run custom deploy script if error early', async () => {
  await expect(() =>
    deploy.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      rootProjectManifest: {
        name: 'root',
        scripts: {
          deploy: 'echo "custom deploy"',
        },
      },
    }, ['deploy'])).rejects.toMatchObject({
    code: 'ERR_PNPM_CANNOT_DEPLOY',
    message: 'A deploy is only possible from inside a workspace',
    hint: 'Maybe you wanted to invoke "pnpm run deploy"',
  })

  await expect(() =>
    deploy.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      workspaceDir: process.cwd(),
      rootProjectManifest: {
        name: 'root',
        scripts: {
          deploy: 'echo "custom deploy"',
        },
      },
    }, ['deploy'])).rejects.toMatchObject({
    code: 'ERR_PNPM_NOTHING_TO_DEPLOY',
    message: 'No project was selected for deployment',
    hint: 'Use --filter to select a project to deploy.\nIn case you want to run the custom "deploy" script in the root manifest, try "pnpm run deploy"',
  })
})

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

// Similar to the test above making sure pnpm deploy works with
// node-linker=hoisted, but we should also make sure not to link projects not in
// the dependency graph of the deployed package.
//
// Let's check node-linker=isolated as well for good measure.
test.each(['isolated', 'hoisted'] as const)(
  'deploy does not link unnecessary workspace packages when node-linker=%p',
  async (nodeLinker) => {
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
        dependencies: {
          'project-2': 'workspace:*',
          'is-positive': '1.0.0',
        },
      },
      {
        name: 'project-2',
        version: '2.0.0',
      },
      {
        name: 'project-3',
        version: '2.0.0',
        dependencies: {
          'is-odd': '1.0.0',
        },
      },
    ])

    const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project-1' }])

    await deploy.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      dev: false,
      production: true,
      recursive: true,
      selectedProjectsGraph,
      nodeLinker,
      sharedWorkspaceLockfile: true,
      lockfileDir: process.cwd(),
      workspaceDir: process.cwd(),
    }, ['dist'])

    const project = assertProject(path.resolve('dist'))

    project.has('project-2')
    project.has('is-positive')

    // project-3 should not be deployed since it's not in the dependency graph of
    // project-1. "is-odd" should not be deployed either since it's only a
    // dependency of project-3.
    project.hasNot('project-3')
    project.hasNot('is-odd')
  }
)

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

  expect(warn).toHaveBeenCalledWith({
    message: expect.stringMatching(/^using --force, deleting deploy pat/),
    prefix: deployFullPath,
  })

  // deployed successfully
  const project = assertProject(deployFullPath)
  project.has('is-positive')
  project.hasNot('is-negative')
  expect(fs.existsSync('deploy/index.js')).toBeTruthy()
  expect(fs.existsSync('pnpm-lock.yaml')).toBeFalsy() // no changes to the lockfile are written

  warn.mockRestore()
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

test('deploy does not preserve the inject workspace packages settings in the lockfile', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '1.0.0',
        private: true,
      },
    },
    {
      name: 'project',
      version: '1.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [{ namePattern: 'project' }])

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    dev: true,
    production: true,
    lockfileOnly: true,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  })

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
  }, ['dist'])

  const project = assertProject(path.resolve('dist'))
  const lockfile = project.readLockfile()
  expect(lockfile.settings).not.toHaveProperty('injectWorkspacePackages')
})
