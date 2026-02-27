import fs from 'fs'
import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { install, type MutatedProject, mutateModules, type ProjectOptions } from '@pnpm/core'
import { type ProjectRootDir } from '@pnpm/types'
import { sync as rimraf } from '@zkochan/rimraf'
import { testDefaults } from '../utils/index.js'

test('using a global virtual store', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    hoistPattern: ['*'],
  }))

  {
    expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
    expect(fs.existsSync(path.resolve('node_modules/.pnpm/lock.yaml'))).toBeTruthy()
    const files = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))
    expect(files).toHaveLength(1)
    expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
    expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
  }

  rimraf('node_modules')
  rimraf(globalVirtualStoreDir)
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    frozenLockfile: true,
    hoistPattern: ['*'],
  }))

  {
    expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
    expect(fs.existsSync(path.resolve('node_modules/.pnpm/lock.yaml'))).toBeTruthy()
    const files = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))
    expect(files).toHaveLength(1)
    expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
    expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
  }
})

test('reinstall from warm global virtual store after deleting node_modules', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    hoistPattern: ['*'],
  })
  await install(manifest, opts)

  // Delete only node_modules, keep the global virtual store warm
  rimraf('node_modules')
  expect(fs.existsSync(globalVirtualStoreDir)).toBeTruthy()

  // Spy on fetchPackage to verify the fast-path skips fetching
  const originalFetchPackage = opts.storeController.fetchPackage
  let fetchPackageCalls = 0
  opts.storeController.fetchPackage = ((...args: Parameters<typeof originalFetchPackage>) => {
    fetchPackageCalls++
    return originalFetchPackage(...args)
  }) as typeof originalFetchPackage

  // Reinstall with frozenLockfile — should reattach from the warm global store
  await install(manifest, {
    ...opts,
    frozenLockfile: true,
  })

  // fetchPackage should NOT be called — all packages reattached from warm GVS
  expect(fetchPackageCalls).toBe(0)

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/lock.yaml'))).toBeTruthy()
  const files = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))
  expect(files).toHaveLength(1)
  expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
})

test('modules are correctly updated when using a global virtual store', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      '@pnpm.e2e/peer-c': '1.0.0',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
  })
  await install(manifest, opts)
  manifest.dependencies['@pnpm.e2e/peer-c'] = '2.0.0'
  await install(manifest, opts)

  {
    expect(fs.existsSync(path.resolve('node_modules/.pnpm/lock.yaml'))).toBeTruthy()
    const files = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/peer-c/2.0.0'))
    expect(files).toHaveLength(1)
    expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/peer-c/2.0.0', files[0], 'node_modules/@pnpm.e2e/peer-c/package.json'))).toBeTruthy()
  }
})

test('injected local packages work with global virtual store', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'project-1': 'workspace:1.0.0',
    },
    dependenciesMeta: {
      'project-1': {
        injected: true,
      },
    },
  }
  preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
    },
  ])
  fs.writeFileSync('project-1/foo.js', '', 'utf8')

  const globalVirtualStoreDir = path.resolve('links')
  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]

  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
    enableGlobalVirtualStore: true,
    dedupeInjectedDeps: false,
    virtualStoreDir: globalVirtualStoreDir,
  }))

  // Verify project-2 has project-1 installed
  expect(fs.existsSync(path.resolve('project-2/node_modules/project-1'))).toBeTruthy()

  // Verify the modules manifest has injectedDeps pointing to global virtual store
  const rootModules = assertProject(process.cwd())
  const modulesState = rootModules.readModulesManifest()
  expect(modulesState?.injectedDeps?.['project-1']).toBeDefined()
  expect(modulesState?.injectedDeps?.['project-1'].length).toBeGreaterThan(0)
  // Injected deps should be in the global virtual store (links directory)
  const injectedDepLocation = modulesState?.injectedDeps?.['project-1'][0]
  expect(injectedDepLocation).toContain('links')
  expect(fs.existsSync(path.join(injectedDepLocation!, 'foo.js'))).toBeTruthy()
})
