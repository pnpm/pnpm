/// <reference path="../../../__typings__/index.d.ts" />
import fs from 'fs'
import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { hashObject } from '@pnpm/crypto.object-hasher'
import { getIndexFilePathInCafs } from '@pnpm/store.cafs'
import { ENGINE_NAME, WANTED_LOCKFILE } from '@pnpm/constants'
import {
  type PackageManifestLog,
  type RootLog,
  type StageLog,
  type StatsLog,
} from '@pnpm/core-loggers'
import { headlessInstall } from '@pnpm/headless'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { tempDir } from '@pnpm/prepare'
import { type DepPath } from '@pnpm/types'
import { getIntegrity } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { sync as rimraf } from '@zkochan/rimraf'
import loadJsonFile from 'load-json-file'
import sinon from 'sinon'
import writeJsonFile from 'write-json-file'
import { testDefaults } from './utils/testDefaults'

const f = fixtures(__dirname)

test('installing a simple project', async () => {
  const prefix = f.prepare('simple')
  const reporter = sinon.spy()

  await headlessInstall(await testDefaults({
    lockfileDir: prefix,
    reporter,
  }))

  const project = assertProject(prefix)
  expect(project.requireModule('is-positive')).toBeTruthy()
  expect(project.requireModule('rimraf')).toBeTruthy()
  expect(project.requireModule('is-negative')).toBeTruthy()
  expect(project.requireModule('colors')).toBeTruthy()

  project.has('.pnpm/colors@1.2.0')

  project.isExecutable('.bin/rimraf')

  expect(project.readCurrentLockfile()).toBeTruthy()
  expect(project.readModulesManifest()).toBeTruthy()

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: loadJsonFile.sync(path.join(prefix, 'package.json')),
  } as PackageManifestLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    added: 15,
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
    removed: 0,
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix,
    stage: 'importing_done',
  } as StageLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: 'is-negative@2.1.0',
    requester: prefix,
    status: 'resolved',
  })).toBeTruthy()

  reporter.resetHistory()
  await headlessInstall(await testDefaults({
    lockfileDir: prefix,
    reporter,
  }))
  // On repeat install no new packages should be added
  // covers https://github.com/pnpm/pnpm/issues/7297
  expect(reporter.calledWithMatch({
    added: 0,
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
  } as StatsLog)).toBeTruthy()
})

test('installing only prod deps', async () => {
  const prefix = f.prepare('simple')

  await headlessInstall(await testDefaults({
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: false,
    },
    lockfileDir: prefix,
  }))

  const project = assertProject(prefix)
  project.has('is-positive')
  project.has('rimraf')
  project.hasNot('is-negative')
  project.hasNot('colors')

  project.isExecutable('.bin/rimraf')
})

test('installing only dev deps', async () => {
  const prefix = f.prepare('simple')

  await headlessInstall(await testDefaults({
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    },
    lockfileDir: prefix,
  }))

  const project = assertProject(prefix)
  project.hasNot('is-positive')
  project.hasNot('rimraf')
  project.has('is-negative')
  project.hasNot('colors')
})

test('installing with package manifest ignored', async () => {
  const prefix = f.prepare('ignore-package-manifest')
  const opt = await testDefaults({
    projects: [],
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: prefix,
  })

  await headlessInstall({ ...opt, ignorePackageManifest: true })

  const project = assertProject(prefix)
  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile.packages).toHaveProperty(['is-positive@1.0.0'])
  expect(currentLockfile.packages).toHaveProperty(['is-negative@2.1.0'])
  project.storeHas('is-negative')
  project.storeHas('is-positive')
  project.hasNot('is-negative')
  project.hasNot('is-positive')
})

test('installing only prod package with package manifest ignored', async () => {
  const prefix = f.prepare('ignore-package-manifest')
  const opt = await testDefaults({
    projects: [],
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: true,
    },
    lockfileDir: prefix,
  })

  await headlessInstall({ ...opt, ignorePackageManifest: true })

  const project = assertProject(prefix)
  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile.packages).not.toHaveProperty(['is-negative@2.1.0'])
  expect(currentLockfile.packages).toHaveProperty(['is-positive@1.0.0'])
  project.storeHasNot('is-negative')
  project.storeHas('is-positive')
  project.hasNot('is-negative')
  project.hasNot('is-positive')
})

test('installing only dev package with package manifest ignored', async () => {
  const prefix = f.prepare('ignore-package-manifest')
  const opt = await testDefaults({
    projects: [],
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    },
    lockfileDir: prefix,
  })

  await headlessInstall({ ...opt, ignorePackageManifest: true })

  const project = assertProject(prefix)
  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile.packages).toHaveProperty(['is-negative@2.1.0'])
  expect(currentLockfile.packages).not.toHaveProperty(['is-positive@1.0.0'])
  project.storeHasNot('is-negative')
  project.storeHas('is-positive')
  project.hasNot('is-negative')
  project.hasNot('is-positive')
})

test('installing non-prod deps then all deps', async () => {
  const prefix = f.prepare('prod-dep-is-dev-subdep')

  await headlessInstall(await testDefaults({
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: prefix,
  }))

  const project = assertProject(prefix)
  const inflight = project.requireModule('inflight')
  expect(typeof inflight).toBe('function')

  project.hasNot('once')

  {
    const currentLockfile = project.readCurrentLockfile()
    expect(currentLockfile.packages).not.toHaveProperty(['is-positive@1.0.0'])
  }

  const reporter = sinon.spy()

  // Repeat normal installation adds missing deps to node_modules
  await headlessInstall(await testDefaults({
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: prefix,
    reporter,
  }))

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'once',
      realName: 'once',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'dev',
      name: 'inflight',
      realName: 'inflight',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeFalsy()

  project.has('once')

  {
    const currentLockfile = project.readCurrentLockfile()
    expect(currentLockfile.packages).toHaveProperty(['is-positive@1.0.0'])
  }
})

test('installing only optional deps', async () => {
  const prefix = f.prepare('simple')

  await headlessInstall(await testDefaults({
    development: false,
    include: {
      dependencies: false,
      devDependencies: false,
      optionalDependencies: true,
    },
    lockfileDir: prefix,
    optional: true,
    production: false,
  }))

  const project = assertProject(prefix)
  project.hasNot('is-positive')
  project.hasNot('rimraf')
  project.hasNot('is-negative')
  project.has('colors')
})

// Covers https://github.com/pnpm/pnpm/issues/1958
test('not installing optional deps', async () => {
  const prefix = f.prepare('simple-with-optional-dep')

  await headlessInstall(await testDefaults({
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: false,
    },
    lockfileDir: prefix,
  }))

  const project = assertProject(prefix)
  project.hasNot('is-positive')
  project.has('@pnpm.e2e/pkg-with-good-optional')
})

test('skipping optional dependency if it cannot be fetched', async () => {
  const prefix = f.prepare('has-nonexistent-optional-dep')
  const reporter = sinon.spy()

  await headlessInstall(await testDefaults({
    lockfileDir: prefix,
    reporter,
  }, {
    retry: {
      retries: 0,
    },
  }))

  const project = assertProject(prefix)
  expect(project.requireModule('is-positive')).toBeTruthy()
  expect(project.requireModule('rimraf')).toBeTruthy()
  expect(project.requireModule('is-negative')).toBeTruthy()

  expect(project.readCurrentLockfile()).toBeTruthy()
  expect(project.readModulesManifest()).toBeTruthy()
})

test('run pre/postinstall scripts', async () => {
  let prefix = f.prepare('deps-have-lifecycle-scripts')
  await using server = await createTestIpcServer(path.join(prefix, 'test.sock'))

  await headlessInstall(await testDefaults({ lockfileDir: prefix }))

  const project = assertProject(prefix)
  const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
  expect(typeof generatedByPreinstall).toBe('function')

  const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
  expect(typeof generatedByPostinstall).toBe('function')

  expect(server.getLines()).toStrictEqual(['install', 'postinstall'])

  prefix = f.prepare('deps-have-lifecycle-scripts')
  server.clear()

  await headlessInstall(await testDefaults({ lockfileDir: prefix, ignoreScripts: true }))

  expect(server.getLines()).toStrictEqual([])

  const nmPath = path.join(prefix, 'node_modules')
  const modulesYaml = await readModulesManifest(nmPath)
  expect(modulesYaml).toBeTruthy()
  expect(modulesYaml!.pendingBuilds).toStrictEqual(['.', '@pnpm.e2e/pre-and-postinstall-scripts-example@2.0.0'])
})

test('orphan packages are removed', async () => {
  const projectDir = f.prepare('simple-with-more-deps')

  await headlessInstall(await testDefaults({
    lockfileDir: projectDir,
  }))

  const simpleDir = f.find('simple')
  fs.copyFileSync(
    path.join(simpleDir, 'package.json'),
    path.join(projectDir, 'package.json')
  )
  fs.copyFileSync(
    path.join(simpleDir, WANTED_LOCKFILE),
    path.join(projectDir, WANTED_LOCKFILE)
  )

  const reporter = sinon.spy()
  await headlessInstall(await testDefaults({
    lockfileDir: projectDir,
    reporter,
  }))

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix: projectDir,
    removed: 1,
  } as StatsLog)).toBeTruthy()

  const project = assertProject(projectDir)
  project.hasNot('resolve-from')
  project.has('rimraf')
  project.has('is-negative')
  project.has('colors')
})

test('available packages are used when node_modules is not clean', async () => {
  const projectDir = tempDir()

  const destPackageJsonPath = path.join(projectDir, 'package.json')
  const destLockfileYamlPath = path.join(projectDir, WANTED_LOCKFILE)

  const hasGlobDir = f.find('has-glob')
  fs.copyFileSync(path.join(hasGlobDir, 'package.json'), destPackageJsonPath)
  fs.copyFileSync(path.join(hasGlobDir, WANTED_LOCKFILE), destLockfileYamlPath)

  await headlessInstall(await testDefaults({ lockfileDir: projectDir }))

  const hasGlobAndRimrafDir = f.find('has-glob-and-rimraf')
  fs.copyFileSync(path.join(hasGlobAndRimrafDir, 'package.json'), destPackageJsonPath)
  fs.copyFileSync(path.join(hasGlobAndRimrafDir, WANTED_LOCKFILE), destLockfileYamlPath)

  const reporter = sinon.spy()
  await headlessInstall(await testDefaults({ lockfileDir: projectDir, reporter }))

  const project = assertProject(projectDir)
  project.has('rimraf')
  project.has('glob')

  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: 'balanced-match@1.0.2',
    requester: projectDir,
    status: 'resolved',
  })).toBeFalsy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: 'rimraf@2.7.1',
    requester: projectDir,
    status: 'resolved',
  })).toBeTruthy()
})

test('available packages are relinked during forced install', async () => {
  const projectDir = tempDir()

  const destPackageJsonPath = path.join(projectDir, 'package.json')
  const destLockfileYamlPath = path.join(projectDir, WANTED_LOCKFILE)

  const hasGlobDir = f.find('has-glob')
  fs.copyFileSync(path.join(hasGlobDir, 'package.json'), destPackageJsonPath)
  fs.copyFileSync(path.join(hasGlobDir, WANTED_LOCKFILE), destLockfileYamlPath)

  await headlessInstall(await testDefaults({ lockfileDir: projectDir }))

  const hasGlobAndRimrafDir = f.find('has-glob-and-rimraf')
  fs.copyFileSync(path.join(hasGlobAndRimrafDir, 'package.json'), destPackageJsonPath)
  fs.copyFileSync(path.join(hasGlobAndRimrafDir, WANTED_LOCKFILE), destLockfileYamlPath)

  const reporter = sinon.spy()
  await headlessInstall(await testDefaults({ lockfileDir: projectDir, reporter, force: true }))

  const project = assertProject(projectDir)
  project.has('rimraf')
  project.has('glob')

  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: 'balanced-match@1.0.2',
    requester: projectDir,
    status: 'resolved',
  })).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: 'rimraf@2.7.1',
    requester: projectDir,
    status: 'resolved',
  })).toBeTruthy()
})

test('installing local dependency', async () => {
  let prefix = f.prepare('has-local-dep')
  f.copy('tar-pkg-1.0.0.tgz', path.join(prefix, 'tar-pkg-1.0.0.tgz'))
  prefix = path.join(prefix, 'pkg')
  const reporter = sinon.spy()

  await headlessInstall(await testDefaults({ lockfileDir: prefix, reporter }))

  const project = assertProject(prefix)
  expect(project.requireModule('tar-pkg'))
})

test('installing local directory dependency', async () => {
  const prefix = f.prepare('has-local-dir-dep')
  const reporter = sinon.spy()

  await headlessInstall(await testDefaults({ lockfileDir: prefix, reporter }))

  const project = assertProject(prefix)
  expect(project.requireModule('example/package.json')).toBeTruthy()
})

test('installing using passed in lockfile files', async () => {
  const prefix = tempDir()

  const simplePkgPath = f.find('simple')
  fs.copyFileSync(path.join(simplePkgPath, 'package.json'), path.join(prefix, 'package.json'))
  fs.copyFileSync(path.join(simplePkgPath, WANTED_LOCKFILE), path.join(prefix, WANTED_LOCKFILE))

  const wantedLockfile = await readWantedLockfile(simplePkgPath, { ignoreIncompatible: false })

  await headlessInstall(await testDefaults({
    lockfileDir: prefix,
    wantedLockfile,
  }))

  const project = assertProject(prefix)

  expect(project.requireModule('is-positive')).toBeTruthy()
  expect(project.requireModule('rimraf')).toBeTruthy()
  expect(project.requireModule('is-negative')).toBeTruthy()
  expect(project.requireModule('colors')).toBeTruthy()
})

test('installation of a dependency that has a resolved peer in subdeps', async () => {
  const prefix = f.prepare('resolved-peer-deps-in-subdeps')

  await headlessInstall(await testDefaults({ lockfileDir: prefix }))

  const project = assertProject(prefix)
  expect(project.requireModule('pnpm-default-reporter')).toBeTruthy()
})

test('install peer dependencies that are in prod dependencies', async () => {
  const prefix = f.prepare('reinstall-peer-deps')

  await headlessInstall(await testDefaults({ lockfileDir: prefix }))

  const project = assertProject(prefix)

  project.has('.pnpm/@pnpm.e2e+peer-a@1.0.1/node_modules/@pnpm.e2e/peer-a')
})

test('installing with hoistPattern=*', async () => {
  const prefix = f.prepare('simple-shamefully-flatten')
  const reporter = jest.fn()

  await headlessInstall(await testDefaults({ lockfileDir: prefix, reporter, hoistPattern: '*' }))

  const project = assertProject(prefix)
  expect(project.requireModule('is-positive')).toBeTruthy()
  expect(project.requireModule('rimraf')).toBeTruthy()
  expect(project.requireModule('.pnpm/node_modules/glob')).toBeTruthy()
  expect(project.requireModule('is-negative')).toBeTruthy()
  expect(project.requireModule('colors')).toBeTruthy()

  project.has('.pnpm/colors@1.2.0')

  project.isExecutable('.bin/rimraf')
  project.isExecutable('.pnpm/node_modules/.bin/hello-world-js-bin')

  expect(project.readCurrentLockfile()).toBeTruthy()
  expect(project.readModulesManifest()).toBeTruthy()

  expect(reporter).toHaveBeenCalledWith(expect.objectContaining({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: expect.objectContaining({
      name: 'simple-shamefully-flatten',
      version: '1.0.0',
    }),
  } as PackageManifestLog))
  expect(reporter).toHaveBeenCalledWith(expect.objectContaining({
    added: 17,
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
  } as StatsLog))
  expect(reporter).toHaveBeenCalledWith(expect.objectContaining({
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
    removed: 0,
  } as StatsLog))
  expect(reporter).toHaveBeenCalledWith(expect.objectContaining({
    level: 'debug',
    name: 'pnpm:stage',
    prefix,
    stage: 'importing_done',
  } as StageLog))
  expect(reporter).toHaveBeenCalledWith(expect.objectContaining({
    level: 'debug',
    packageId: 'is-negative@2.1.0',
    requester: prefix,
    status: 'resolved',
  }))

  const modules = project.readModulesManifest()

  expect(modules!.hoistedDependencies['balanced-match@1.0.2' as DepPath]).toStrictEqual({ 'balanced-match': 'private' })
})

test('installing with publicHoistPattern=*', async () => {
  const prefix = f.prepare('simple-shamefully-flatten')
  const reporter = sinon.spy()

  await headlessInstall(await testDefaults({ lockfileDir: prefix, reporter, publicHoistPattern: '*' }))

  const project = assertProject(prefix)
  expect(project.requireModule('is-positive')).toBeTruthy()
  expect(project.requireModule('rimraf')).toBeTruthy()
  expect(project.requireModule('glob')).toBeTruthy()
  expect(project.requireModule('is-negative')).toBeTruthy()
  expect(project.requireModule('colors')).toBeTruthy()

  project.has('.pnpm/colors@1.2.0')

  project.isExecutable('.bin/rimraf')
  project.isExecutable('.bin/hello-world-js-bin')

  expect(project.readCurrentLockfile()).toBeTruthy()
  expect(project.readModulesManifest()).toBeTruthy()

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: loadJsonFile.sync(path.join(prefix, 'package.json')),
  } as PackageManifestLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    added: 17,
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
    removed: 0,
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix,
    stage: 'importing_done',
  } as StageLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: 'is-negative@2.1.0',
    requester: prefix,
    status: 'resolved',
  })).toBeTruthy()

  const modules = project.readModulesManifest()

  expect(modules!.hoistedDependencies['balanced-match@1.0.2' as DepPath]).toStrictEqual({ 'balanced-match': 'public' })
})

test('installing with publicHoistPattern=* in a project with external lockfile', async () => {
  const lockfileDir = f.prepare('pkg-with-external-lockfile')
  const prefix = path.join(lockfileDir, 'pkg')

  await headlessInstall(await testDefaults({
    lockfileDir,
    projects: [prefix],
    publicHoistPattern: '*',
  }))

  const project = assertProject(lockfileDir)
  expect(project.requireModule('accepts')).toBeTruthy()
})

const ENGINE_DIR = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

test.each([['isolated'], ['hoisted']])('using side effects cache with nodeLinker=%s', async (nodeLinker) => {
  let prefix = f.prepare('side-effects')

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  const opts = await testDefaults({
    lockfileDir: prefix,
    nodeLinker,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  await headlessInstall(opts)

  const cacheIntegrityPath = getIndexFilePathInCafs(opts.storeDir, getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0'), '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0')
  const cacheIntegrity = loadJsonFile.sync<any>(cacheIntegrityPath) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(cacheIntegrity!.sideEffects).toBeTruthy()
  const sideEffectsKey = `${ENGINE_NAME};deps=${hashObject({
    id: `@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0:${getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0')}`,
    deps: {
      '@pnpm.e2e/hello-world-js-bin': hashObject({
        id: `@pnpm.e2e/hello-world-js-bin@1.0.0:${getIntegrity('@pnpm.e2e/hello-world-js-bin', '1.0.0')}`,
        deps: {},
      }),
    },
  })}`
  expect(cacheIntegrity).toHaveProperty(['sideEffects', sideEffectsKey, 'added', 'generated-by-postinstall.js'])
  delete cacheIntegrity!.sideEffects[sideEffectsKey].added['generated-by-postinstall.js']

  expect(cacheIntegrity).toHaveProperty(['sideEffects', sideEffectsKey, 'added', 'generated-by-preinstall.js'])
  writeJsonFile.sync(cacheIntegrityPath, cacheIntegrity)

  prefix = f.prepare('side-effects')
  const opts2 = await testDefaults({
    lockfileDir: prefix,
    nodeLinker,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    storeDir: opts.storeDir,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  await headlessInstall(opts2)

  expect(fs.existsSync(path.join(prefix, 'node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'))).toBeFalsy()
  expect(fs.existsSync(path.join(prefix, 'node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'))).toBeTruthy()
})

test.skip('using side effects cache and hoistPattern=*', async () => {
  const lockfileDir = f.prepare('side-effects-of-subdep')

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  const opts = await testDefaults({
    hoistPattern: '*',
    lockfileDir,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  await headlessInstall(opts)

  const project = assertProject(lockfileDir)
  project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created

  const cacheBuildDir = path.join(opts.storeDir, `diskusage@1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  fs.writeFileSync(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  rimraf(path.join(lockfileDir, 'node_modules'))
  await headlessInstall(opts)

  expect(fs.existsSync(path.join(lockfileDir, 'node_modules/.pnpm/node_modules/diskusage/build/new-file.txt'))).toBeTruthy()

  project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created
})

test('installing in a workspace', async () => {
  const workspaceFixture = f.prepare('workspace')

  const projects = [
    path.join(workspaceFixture, 'foo'),
    path.join(workspaceFixture, 'bar'),
  ]

  await headlessInstall(await testDefaults({
    lockfileDir: workspaceFixture,
    projects,
  }))

  const projectBar = assertProject(path.join(workspaceFixture, 'bar'))

  projectBar.has('foo')

  await headlessInstall(await testDefaults({
    lockfileDir: workspaceFixture,
    projects: [projects[0]],
  }))

  const rootModules = assertProject(workspaceFixture)
  const lockfile = rootModules.readCurrentLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    'is-negative@1.0.0',
    'is-positive@1.0.0',
  ])
})

test('installing with no symlinks but with PnP', async () => {
  const prefix = f.prepare('simple')

  await headlessInstall(await testDefaults({
    enablePnp: true,
    lockfileDir: prefix,
    symlink: false,
  }))

  expect([...fs.readdirSync(path.join(prefix, 'node_modules'))]).toStrictEqual(['.bin', '.modules.yaml', '.pnpm'])
  expect([...fs.readdirSync(path.join(prefix, 'node_modules/.pnpm/rimraf@2.7.1/node_modules'))]).toStrictEqual(['rimraf'])

  const project = assertProject(prefix)
  expect(project.readCurrentLockfile()).toBeTruthy()
  expect(project.readModulesManifest()).toBeTruthy()
  expect(fs.existsSync(path.join(prefix, '.pnp.cjs'))).toBeTruthy()
})

test('installing with no modules directory', async () => {
  const prefix = f.prepare('simple')

  await headlessInstall(await testDefaults({
    enableModulesDir: false,
    lockfileDir: prefix,
  }))

  expect(fs.existsSync(path.join(prefix, 'node_modules'))).toBeFalsy()
})

test('installing with no modules directory and a patched dependency', async () => {
  const prefix = f.prepare('simple-with-patch')

  await headlessInstall(await testDefaults({
    enableModulesDir: false,
    lockfileDir: prefix,
  }))

  expect(fs.existsSync(path.join(prefix, 'node_modules'))).toBeFalsy()
})

test('installing with node-linker=hoisted', async () => {
  const prefix = f.prepare('has-several-versions-of-same-pkg')

  await headlessInstall(await testDefaults({
    enableModulesDir: false,
    lockfileDir: prefix,
    nodeLinker: 'hoisted',
  }))

  expect(fs.realpathSync('node_modules/ms')).toBe(path.resolve('node_modules/ms'))
  expect(fs.realpathSync('node_modules/send')).toBe(path.resolve('node_modules/send'))
  expect(fs.existsSync('node_modules/send/node_modules/ms')).toBeTruthy()
})

test('installing in a workspace with node-linker=hoisted', async () => {
  const prefix = f.prepare('workspace2')

  await headlessInstall(await testDefaults({
    lockfileDir: prefix,
    nodeLinker: 'hoisted',
    projects: [
      path.join(prefix, 'foo'),
      path.join(prefix, 'bar'),
    ],
  }))

  expect(fs.realpathSync('bar/node_modules/foo')).toBe(path.resolve('foo'))
  expect(readPkgVersion(path.join(prefix, 'foo/node_modules/webpack'))).toBe('2.7.0')
  expect(fs.realpathSync('foo/node_modules/express')).toBe(path.resolve('foo/node_modules/express'))
  expect(readPkgVersion(path.join(prefix, 'foo/node_modules/express'))).toBe('4.17.2')
  expect(readPkgVersion(path.join(prefix, 'node_modules/webpack'))).toBe('5.65.0')
  expect(readPkgVersion(path.join(prefix, 'node_modules/express'))).toBe('2.5.11')
})

function readPkgVersion (dir: string): string {
  return loadJsonFile.sync<{ version: string }>(path.join(dir, 'package.json')).version
}

test('installing a package deeply installs all required dependencies', async () => {
  const workspaceFixture = f.prepare('workspace-external-depends-deep')
  const projects = [
    path.join(workspaceFixture),
    path.join(workspaceFixture, 'packages/f'),
    path.join(workspaceFixture, 'packages/g'),
    workspaceFixture,
  ]

  await headlessInstall(
    await testDefaults({
      lockfileDir: workspaceFixture,
      projects,
      selectedProjectDirs: [projects[2]],
    })
  )

  for (const projectDir of projects) {
    if (projectDir === workspaceFixture) {
      continue
    }
    const projectAssertion = assertProject(projectDir)
    expect(projectAssertion.requireModule('is-positive')).toBeTruthy()
  }
})
