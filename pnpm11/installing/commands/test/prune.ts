import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { assertProject } from '@pnpm/assert-project'
import { add, install, prune } from '@pnpm/installing.commands'
import { prepare } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { symlinkDirSync } from 'symlink-dir'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`
const f = fixtures(import.meta.dirname)

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  excludeLinksFromLockfile: false,
  extraEnv: {},
  cliOptions: {},
  deployAllFiles: false,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  linkWorkspacePackages: true,
  pnpmfile: ['.pnpmfile.cjs'],
  pnpmHomeDir: '',
  preferWorkspacePackages: true,
  configByUri: {},
  registries: {
    default: REGISTRY_URL,
  },
  rootProjectManifestDir: '',
  sort: true,
  userConfig: {},
  workspaceConcurrency: 1,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

test('prune removes external link that is not in package.json', async () => {
  const project = prepare(undefined)
  const storeDir = path.resolve('store')
  f.copy('local-pkg', 'local')

  symlinkDirSync(path.resolve('local'), path.join('node_modules/local-pkg'))

  project.has('local-pkg')

  await prune.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    storeDir,
  })

  project.hasNot('local-pkg')
})

test('prune keeps hoisted dependencies', async () => {
  const project = prepare(undefined)
  const storeDir = path.resolve('store')
  const cacheDir = path.resolve('cache')

  await add.handler({
    ...DEFAULT_OPTIONS,
    cacheDir,
    dir: process.cwd(),
    storeDir,
  }, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'])

  await prune.handler({
    ...DEFAULT_OPTIONS,
    cacheDir,
    dir: process.cwd(),
    storeDir,
  })

  project.hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
})

test('prune removes dev dependencies', async () => {
  const project = prepare({
    dependencies: { 'is-positive': '1.0.0' },
    devDependencies: { 'is-negative': '1.0.0' },
  })
  const storeDir = path.resolve('store')

  await install.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    linkWorkspacePackages: true,
    storeDir,
  })

  await prune.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dev: false,
    dir: process.cwd(),
    storeDir,
  })

  project.has('is-positive')
  project.has('.pnpm/is-positive@1.0.0')
  project.hasNot('is-negative')
  project.hasNot('.pnpm/is-negative@1.0.0')
})

test('prune: ignores all the lifecycle scripts when --ignore-scripts is used', async () => {
  await using server = await createTestIpcServer()

  prepare({
    name: 'test-prune-with-ignore-scripts',
    version: '0.0.0',

    scripts: {
      // eslint-disable:object-literal-sort-keys
      preinstall: server.sendLineScript('preinstall'),
      prepare: server.sendLineScript('prepare'),
      postinstall: server.sendLineScript('postinstall'),
      // eslint-enable:object-literal-sort-keys
    },
  })

  const storeDir = path.resolve('store')

  const opts = {
    ...DEFAULT_OPTIONS,
    ignoreScripts: true,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    linkWorkspacePackages: true,
    storeDir,
  }

  await install.handler(opts)

  await prune.handler(opts)

  expect(fs.existsSync('package.json')).toBeTruthy()
  expect(server.getLines()).toStrictEqual([])
})

test('cliOptionsTypes', () => {
  expect(prune.cliOptionsTypes()).toHaveProperty('production')
  expect(prune.cliOptionsTypes()).toHaveProperty('dev')
  expect(prune.cliOptionsTypes()).toHaveProperty('ignore-scripts')
  expect(prune.cliOptionsTypes()).toHaveProperty('optional')
})

test('prune --prod in workspace keeps workspace package symlink', async () => {
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  // Set up workspace directory structure manually
  const rootDir = process.cwd()
  const appDir = path.join(rootDir, 'apps/app')
  const libDir = path.join(rootDir, 'packages/lib')

  fs.mkdirSync(appDir, { recursive: true })
  fs.mkdirSync(libDir, { recursive: true })

  const rootManifest = {
    name: 'root',
    private: true,
  }
  const libManifest = {
    name: '@scope/lib',
    version: '1.0.0',
    main: 'index.js',
  }
  const appManifest = {
    name: 'app',
    version: '1.0.0',
    dependencies: {
      '@scope/lib': 'workspace:*',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
  }

  fs.writeFileSync(path.join(rootDir, 'package.json'), JSON.stringify(rootManifest, null, 2))
  fs.writeFileSync(path.join(libDir, 'package.json'), JSON.stringify(libManifest, null, 2))
  fs.writeFileSync(path.join(libDir, 'index.js'), 'module.exports = "lib"')
  fs.writeFileSync(path.join(appDir, 'package.json'), JSON.stringify(appManifest, null, 2))
  fs.writeFileSync(path.join(rootDir, 'pnpm-workspace.yaml'), 'packages:\n  - "apps/*"\n  - "packages/*"')

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const allProjects: any[] = [
    {
      manifest: rootManifest,
      rootDir: rootDir,
      rootDirRealPath: fs.realpathSync(rootDir),
      writeProjectManifest: async (_manifest: unknown) => {},
    },
    {
      manifest: libManifest,
      rootDir: libDir,
      rootDirRealPath: fs.realpathSync(libDir),
      writeProjectManifest: async (_manifest: unknown) => {},
    },
    {
      manifest: appManifest,
      rootDir: appDir,
      rootDirRealPath: fs.realpathSync(appDir),
      writeProjectManifest: async (_manifest: unknown) => {},
    },
  ]
  const selectedProjectsGraph = {
    [rootDir]: {
      dependencies: [],
      package: allProjects[0],
    },
    [libDir]: {
      dependencies: [],
      package: allProjects[1],
    },
    [appDir]: {
      dependencies: [],
      package: allProjects[2],
    },
  }

  // Install
  await install.handler({
    ...DEFAULT_OPTIONS,
    allProjects: allProjects as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    cacheDir,
    dir: rootDir,
    storeDir,
    workspaceDir: rootDir,
    lockfileDir: rootDir,
    selectedProjectsGraph: selectedProjectsGraph as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    recursive: true,
    confirmModulesPurge: false,
  })

  // Check that the symlink exists
  const appProject = assertProject(appDir)
  appProject.has('@scope/lib')
  appProject.has('is-positive')

  // Prune
  await prune.handler({
    ...DEFAULT_OPTIONS,
    allProjects: allProjects as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    cacheDir,
    dir: rootDir,
    storeDir,
    workspaceDir: rootDir,
    lockfileDir: rootDir,
    selectedProjectsGraph: selectedProjectsGraph as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    production: true,
    dev: false,
    recursive: true,
    confirmModulesPurge: false,
  })

  // Check that devDependencies were pruned, but the workspace symlink remains
  appProject.has('@scope/lib')
  appProject.hasNot('is-positive')
})
