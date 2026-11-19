import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { PnpmError } from '@pnpm/error'
import { addDependenciesToPackage, install, type MutatedProject, mutateModules } from '@pnpm/installing.deps-installer'
import type { Modules } from '@pnpm/installing.modules-yaml'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import type { ProjectRootDir } from '@pnpm/types'
import { rimrafSync } from '@zkochan/rimraf'
import { isCI } from 'ci-info'

import { validateModules } from '../src/install/validateModules.js'
import { testDefaults } from './utils/index.js'

test('fail on non-compatible node_modules', async () => {
  prepareEmpty()
  const opts = testDefaults()

  await saveModulesYaml('0.50.0', opts.storeDir)

  try {
    await addDependenciesToPackage({}, ['is-negative'], opts)
    throw new Error('should have failed')
  } catch (err: any) { // eslint-disable-line
    expect(err.code).toBe('ERR_PNPM_MODULES_BREAKING_CHANGE')
  }
})

test('fail when adding a dependency after changing nodeLinker', async () => {
  prepareEmpty()
  await install({}, testDefaults({ nodeLinker: 'hoisted' }))

  await expect(addDependenciesToPackage({}, ['is-positive@1.0.0'], testDefaults({
    confirmModulesPurge: false,
    nodeLinker: 'isolated',
  }))).rejects.toMatchObject({
    code: 'ERR_PNPM_NODE_LINKER_DIFF',
    message: 'This modules directory was created using a different node-linker value. Run "pnpm install" to recreate the modules directory.',
  })
})

test('a plain install recreates the modules directory after changing nodeLinker', async () => {
  prepareEmpty()
  const manifest = { dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' } }
  const hoistedDep = path.resolve('node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')
  await install(manifest, testDefaults({ nodeLinker: 'hoisted' }))
  expect(fs.existsSync(hoistedDep)).toBe(true)

  await install(manifest, testDefaults({ confirmModulesPurge: false, nodeLinker: 'isolated' }))

  expect(fs.existsSync(hoistedDep)).toBe(false)
})

test.each([
  ['nodeLinker', { nodeLinker: 'hoisted' }, { nodeLinker: 'isolated' }, 'ERR_PNPM_NODE_LINKER_DIFF', 'node_modules/is-negative'],
  ['virtualStoreDirMaxLength', {}, { virtualStoreDirMaxLength: 60 }, 'ERR_PNPM_VIRTUAL_STORE_DIR_MAX_LENGTH_DIFF', 'project-2/node_modules/is-negative/package.json'],
  ['publicHoistPattern', {}, { publicHoistPattern: ['is-*'] }, 'ERR_PNPM_PUBLIC_HOIST_PATTERN_DIFF', 'project-2/node_modules/is-negative/package.json'],
  ['hoistPattern', {}, { hoistPattern: ['is-*'] }, 'ERR_PNPM_HOIST_PATTERN_DIFF', 'project-2/node_modules/is-negative/package.json'],
] as const)('fail when a root-only install follows a %s change in a workspace', async (_setting, before, after, code, project2Dep) => {
  preparePackages([
    {
      location: '.',
      package: { name: 'root' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2', dependencies: { 'is-negative': '1.0.0' } },
    },
  ])
  const allProjects = [
    {
      buildIndex: 0,
      manifest: { name: 'root' },
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: { name: 'project-2', dependencies: { 'is-negative': '1.0.0' } },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const mutations: MutatedProject[] = allProjects.map(({ rootDir }) => ({ mutation: 'install', rootDir }))
  await mutateModules(mutations, testDefaults({ allProjects, ...before }))
  expect(fs.existsSync(project2Dep)).toBe(true)

  await expect(mutateModules(mutations.slice(0, 1), testDefaults({
    allProjects,
    confirmModulesPurge: false,
    ...after,
  }))).rejects.toMatchObject({ code })

  expect(fs.existsSync(project2Dep)).toBe(true)
})

test('a whole-workspace install recreates the modules directory after a virtualStoreDirMaxLength change', async () => {
  preparePackages([
    {
      location: '.',
      package: { name: 'root' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2', dependencies: { 'is-negative': '1.0.0' } },
    },
  ])
  const allProjects = [
    {
      buildIndex: 0,
      manifest: { name: 'root' },
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: { name: 'project-2', dependencies: { 'is-negative': '1.0.0' } },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const mutations: MutatedProject[] = allProjects.map(({ rootDir }) => ({ mutation: 'install', rootDir }))
  await mutateModules(mutations, testDefaults({ allProjects }))
  fs.writeFileSync('node_modules/sentinel', '')

  await mutateModules(mutations, testDefaults({ allProjects, confirmModulesPurge: false, virtualStoreDirMaxLength: 60 }))

  expect(fs.existsSync('node_modules/sentinel')).toBe(false)
  expect(fs.existsSync('project-2/node_modules/is-negative/package.json')).toBe(true)
})

test("don't fail on non-compatible node_modules when forced", async () => {
  prepareEmpty()
  const opts = testDefaults({ force: true })

  await saveModulesYaml('0.50.0', opts.storeDir)

  await install({}, opts)
})

test('forced modules repair never purges the project root', async () => {
  prepareEmpty()
  const sentinel = path.resolve('keep.txt')
  fs.writeFileSync(sentinel, 'keep')
  fs.writeFileSync('.modules.yaml', 'packageManager: pnpm@3\nlayoutVersion: 1\n')

  await expect(install({}, testDefaults({
    confirmModulesPurge: false,
    force: true,
    modulesDir: '.',
  }))).rejects.toMatchObject({ code: 'ERR_PNPM_UNSAFE_MODULES_DIR' })

  expect(fs.readFileSync(sentinel, 'utf8')).toBe('keep')
})

test('forced modules repair never follows a modules directory symlink', async () => {
  prepareEmpty()
  const projectRoot = process.cwd() as ProjectRootDir
  const modulesDir = path.join(projectRoot, 'linked-modules')
  const sentinel = path.join(projectRoot, 'keep.txt')
  fs.writeFileSync(sentinel, 'keep')
  fs.symlinkSync(projectRoot, modulesDir, process.platform === 'win32' ? 'junction' : 'dir')
  const opts = testDefaults()

  try {
    await expect(validateModules({ virtualStoreDirMaxLength: 1 } as Modules, [{
      id: '.',
      modulesDir,
      rootDir: projectRoot,
    }], {
      confirmModulesPurge: false,
      forceNewModules: true,
      lockfileDir: projectRoot,
      modulesDir,
      nodeLinker: opts.nodeLinker ?? 'isolated',
      mutatedProjectDirs: new Set([projectRoot]),
      registriesByScope: opts.registriesByScope,
      storeDir: opts.storeDir,
      virtualStoreDir: path.join(modulesDir, '.pnpm'),
      virtualStoreDirMaxLength: 2,
    })).rejects.toMatchObject({ code: 'ERR_PNPM_UNSAFE_MODULES_DIR' })

    expect(fs.readFileSync(sentinel, 'utf8')).toBe('keep')
  } finally {
    fs.unlinkSync(modulesDir)
  }
})

test("don't fail on non-compatible node_modules when forced in a workspace", async () => {
  preparePackages([
    {
      location: 'pkg',
      package: {},
    },
  ])
  const opts = testDefaults({ force: true })

  process.chdir('pkg')
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-positive@1.0.0'], testDefaults({ lockfileDir: path.resolve('..') }))
  rimrafSync('node_modules')

  process.chdir('..')

  fs.writeFileSync('node_modules/.modules.yaml', `packageManager: pnpm@${3}\nstore: ${opts.storeDir}\nlayoutVersion: 1`)

  await install(manifest, { ...opts, dir: path.resolve('pkg'), lockfileDir: process.cwd() })
})

test('do not fail on non-compatible node_modules when forced with a named installation', async () => {
  prepareEmpty()
  const opts = testDefaults()

  await saveModulesYaml('0.50.0', opts.storeDir)

  let err!: PnpmError
  try {
    await addDependenciesToPackage({}, ['is-negative'], opts)
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_MODULES_BREAKING_CHANGE')

  await install({}, {
    ...opts,
    confirmModulesPurge: false,
  })
})

test("don't fail on non-compatible store when forced", async () => {
  prepareEmpty()
  const opts = testDefaults({ force: true })

  await saveModulesYaml('0.32.0', opts.storeDir)

  await install({}, opts)
})

test('do not fail on non-compatible store when forced during named installation', async () => {
  prepareEmpty()
  const opts = testDefaults()

  await saveModulesYaml('0.32.0', opts.storeDir)

  let err!: PnpmError
  try {
    await addDependenciesToPackage({}, ['is-negative'], opts)
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_MODULES_BREAKING_CHANGE')

  await install({}, {
    ...opts,
    confirmModulesPurge: false,
  })
})

test('do not fail on non-compatible node_modules in non-TTY environment', async () => {
  prepareEmpty()
  const opts = testDefaults()

  await saveModulesYaml('0.50.0', opts.storeDir)
  fs.writeFileSync('node_modules/sentinel.txt', 'sentinel')

  await expect(addDependenciesToPackage({}, ['is-negative'], opts)).rejects.toMatchObject({
    code: 'ERR_PNPM_MODULES_BREAKING_CHANGE',
  })

  await withoutTTY(() => install({}, { ...opts, confirmModulesPurge: true }))

  expect(fs.existsSync('node_modules/sentinel.txt')).toBe(false)
})

test('fail on non-compatible custom modules directory in non-TTY environment', async () => {
  prepareEmpty()
  const opts = testDefaults()

  await saveModulesYaml('0.50.0', opts.storeDir, 'custom_modules')

  await expect(withoutTTY(() => install({}, {
    ...opts,
    confirmModulesPurge: true,
    modulesDir: 'custom_modules',
  }))).rejects.toMatchObject({
    code: 'ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_UNSAFE',
    message: expect.stringContaining(path.resolve('custom_modules')),
  })
})

test('fail on non-compatible symlinked node_modules in non-TTY environment', async () => {
  prepareEmpty()
  const opts = testDefaults()

  fs.mkdirSync('real_modules')
  fs.symlinkSync('real_modules', 'node_modules', process.platform === 'win32' ? 'junction' : 'dir')
  await saveModulesYaml('0.50.0', opts.storeDir, 'real_modules')

  await expect(withoutTTY(() => install({}, { ...opts, confirmModulesPurge: true }))).rejects.toMatchObject({
    code: 'ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_UNSAFE',
  })
})

test('a filtered install with other dependency groups keeps the modules directories of skipped projects', async () => {
  preparePackages([
    { location: '.', package: { name: 'root' } },
    { location: 'project-1', package: { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } } },
    { location: 'project-2', package: { name: 'project-2', dependencies: { 'is-negative': '1.0.0' } } },
  ])
  const allProjects = [
    { buildIndex: 0, manifest: { name: 'root' }, rootDir: process.cwd() as ProjectRootDir },
    { buildIndex: 0, manifest: { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } }, rootDir: path.resolve('project-1') as ProjectRootDir },
    { buildIndex: 0, manifest: { name: 'project-2', dependencies: { 'is-negative': '1.0.0' } }, rootDir: path.resolve('project-2') as ProjectRootDir },
  ]
  const mutations: MutatedProject[] = allProjects.map(({ rootDir }) => ({ mutation: 'install', rootDir }))
  await mutateModules(mutations, testDefaults({ allProjects }))
  const skippedProjectDep = path.resolve('project-2/node_modules/is-negative')
  expect(fs.existsSync(skippedProjectDep)).toBe(true)

  await withoutTTY(() => mutateModules([mutations[1]], testDefaults({
    allProjects,
    confirmModulesPurge: true,
    include: { dependencies: true, devDependencies: false, optionalDependencies: true },
  })))

  expect(fs.existsSync(path.resolve('project-1/node_modules/is-positive'))).toBe(true)
  expect(fs.existsSync(skippedProjectDep)).toBe(true)
})

test('a filtered install with other dependency groups keeps the shared virtual store of a workspace without a root project', async () => {
  preparePackages([
    { location: 'project-1', package: { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } } },
    { location: 'project-2', package: { name: 'project-2', dependencies: { 'is-negative': '1.0.0' } } },
  ])
  const allProjects = [
    { buildIndex: 0, manifest: { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } }, rootDir: path.resolve('project-1') as ProjectRootDir },
    { buildIndex: 0, manifest: { name: 'project-2', dependencies: { 'is-negative': '1.0.0' } }, rootDir: path.resolve('project-2') as ProjectRootDir },
  ]
  const mutations: MutatedProject[] = allProjects.map(({ rootDir }) => ({ mutation: 'install', rootDir }))
  await mutateModules(mutations, testDefaults({ allProjects }))
  const skippedProjectDep = path.resolve('project-2/node_modules/is-negative/package.json')
  expect(fs.existsSync(skippedProjectDep)).toBe(true)

  await withoutTTY(() => mutateModules([mutations[0]], testDefaults({
    allProjects,
    confirmModulesPurge: true,
    include: { dependencies: true, devDependencies: false, optionalDependencies: true },
  })))

  expect(fs.existsSync(path.resolve('project-1/node_modules/is-positive/package.json'))).toBe(true)
  expect(fs.existsSync(skippedProjectDep)).toBe(true)
})

async function withoutTTY<Result> (fn: () => Promise<Result>): Promise<Result> {
  const originalIsTTY = process.stdin.isTTY
  Object.defineProperty(process.stdin, 'isTTY', { value: false, configurable: true })
  try {
    return await fn()
  } finally {
    Object.defineProperty(process.stdin, 'isTTY', { value: originalIsTTY, configurable: true })
  }
}

async function saveModulesYaml (pnpmVersion: string, storeDir: string, dir: string = 'node_modules') {
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir)
  }
  fs.writeFileSync(path.join(dir, '.modules.yaml'), `packageManager: pnpm@${pnpmVersion}\nstoreDir: ${storeDir}`)
}

test(`fail on non-compatible ${WANTED_LOCKFILE} when frozen lockfile installation is used`, async () => {
  if (isCI) {
    console.log('this test will always fail on CI servers')
    return
  }

  prepareEmpty()
  fs.writeFileSync(WANTED_LOCKFILE, '')

  try {
    await addDependenciesToPackage({}, ['is-negative'], testDefaults({ frozenLockfile: true }))
    throw new Error('should have failed')
  } catch (err: any) { // eslint-disable-line
    if (err.message === 'should have failed') throw err
    expect(err.code).toBe('ERR_PNPM_BROKEN_LOCKFILE')
  }
})

test(`don't fail on non-compatible ${WANTED_LOCKFILE} when forced`, async () => {
  prepareEmpty()
  fs.writeFileSync(WANTED_LOCKFILE, '')

  await addDependenciesToPackage({}, ['is-negative'], testDefaults({ force: true }))
})
