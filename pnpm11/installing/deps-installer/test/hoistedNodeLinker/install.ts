import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { addDependenciesToPackage, install, mutateModules, mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/testing.registry-mock'
import type { ProjectRootDir } from '@pnpm/types'
import { rimrafSync } from '@zkochan/rimraf'
import { loadJsonFileSync } from 'load-json-file'
import { readYamlFileSync } from 'read-yaml-file'
import { symlinkDir } from 'symlink-dir'

import { testDefaults } from '../utils/index.js'

test('installing with hoisted node-linker', async () => {
  prepareEmpty()

  const manifest = {
    dependencies: {
      send: '0.17.2',
      'has-flag': '1.0.0',
      ms: '1.0.0',
    },
  }
  await install(manifest, testDefaults({
    nodeLinker: 'hoisted',
  }))

  expect(fs.realpathSync('node_modules/send')).toEqual(path.resolve('node_modules/send'))
  expect(fs.realpathSync('node_modules/has-flag')).toEqual(path.resolve('node_modules/has-flag'))
  expect(fs.realpathSync('node_modules/ms')).toEqual(path.resolve('node_modules/ms'))
  expect(fs.existsSync('node_modules/send/node_modules/ms')).toBeTruthy()

  expect(readYamlFileSync<{ nodeLinker: string }>('node_modules/.modules.yaml').nodeLinker).toBe('hoisted')

  // If a package from node_modules is removed, it should be re-added.
  rimrafSync('node_modules/send')
  await install(manifest, testDefaults({
    nodeLinker: 'hoisted',
  }))
  expect(fs.realpathSync('node_modules/send')).toEqual(path.resolve('node_modules/send'))
})

test('installing with hoisted node-linker and no lockfile', async () => {
  prepareEmpty()

  await install({
    dependencies: {
      ms: '1.0.0',
    },
  }, testDefaults({
    useLockfile: false,
    nodeLinker: 'hoisted',
  }))

  expect(fs.realpathSync('node_modules/ms')).toEqual(path.resolve('node_modules/ms'))
  expect(fs.existsSync('pnpm-lock.yaml')).toBeFalsy()
})

test('overwriting (@pnpm.e2e/multi-version-b@3.0.0 with @pnpm.e2e/multi-version-b@latest)', async () => {
  await addDistTag({ package: '@pnpm.e2e/multi-version-b', version: '3.1.0', distTag: 'latest' })
  const project = prepareEmpty()
  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/multi-version-b@3.0.0'],
    testDefaults({ nodeLinker: 'hoisted', save: true })
  )

  project.storeHas('@pnpm.e2e/multi-version-b', '3.0.0')

  const { updatedManifest } = await addDependenciesToPackage(
    manifest,
    ['@pnpm.e2e/multi-version-b@latest'],
    testDefaults({ nodeLinker: 'hoisted', save: true })
  )

  project.storeHas('@pnpm.e2e/multi-version-b', '3.1.0')
  expect(updatedManifest.dependencies?.['@pnpm.e2e/multi-version-b']).toBe('3.1.0')
  expect(loadJsonFileSync<{ version: string }>('node_modules/@pnpm.e2e/multi-version-b/package.json').version).toBe('3.1.0')
})

test('overwriting existing files in node_modules', async () => {
  prepareEmpty()
  await symlinkDir(import.meta.dirname, path.resolve('node_modules/is-positive'))

  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['is-positive@3.0.0'],
    testDefaults({ nodeLinker: 'hoisted', save: true })
  )

  expect(manifest.dependencies?.['is-positive']).toBe('3.0.0')
  expect(loadJsonFileSync<{ version: string }>('node_modules/is-positive/package.json').version).toBe('3.0.0')
})

test('preserve subdeps on update', async () => {
  prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.0', distTag: 'latest' })

  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/foobarqar@1.0.0', '@pnpm.e2e/bar@100.1.0'],
    testDefaults({ nodeLinker: 'hoisted' })
  )

  await addDependenciesToPackage(
    manifest,
    ['@pnpm.e2e/foobarqar@1.0.1'],
    testDefaults({ nodeLinker: 'hoisted' })
  )

  expect(loadJsonFileSync<{ version: string }>('node_modules/@pnpm.e2e/bar/package.json').version).toBe('100.1.0')
  expect(loadJsonFileSync<{ version: string }>('node_modules/@pnpm.e2e/foobarqar/package.json').version).toBe('1.0.1')
  expect(loadJsonFileSync<{ version: string }>('node_modules/@pnpm.e2e/foobarqar/node_modules/@pnpm.e2e/bar/package.json').version).toBe('100.0.0')
})

test('adding a new dependency to one of the workspace projects', async () => {
  prepareEmpty()

  let [{ manifest }] = (await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects: [
      {
        buildIndex: 0,
        manifest: {
          name: 'project-1',
          version: '1.0.0',

          dependencies: {
            '@pnpm.e2e/bar': '100.0.0',
          },
        },
        rootDir: path.resolve('project-1') as ProjectRootDir,
      },
      {
        buildIndex: 1,
        manifest: {
          name: 'project-2',
          version: '1.0.0',

          dependencies: {
            '@pnpm.e2e/foobarqar': '1.0.0',
          },
        },
        rootDir: path.resolve('project-2') as ProjectRootDir,
      },
    ],
    nodeLinker: 'hoisted',
  }))).updatedProjects
  manifest = (await addDependenciesToPackage(
    manifest,
    ['is-negative@1.0.0'],
    testDefaults({ nodeLinker: 'hoisted', prefix: path.resolve('project-1'), targetDependenciesField: 'devDependencies' })
  )).updatedManifest

  expect(manifest.dependencies).toStrictEqual({ '@pnpm.e2e/bar': '100.0.0' })
  expect(manifest.devDependencies).toStrictEqual({ 'is-negative': '1.0.0' })
  expect(loadJsonFileSync<{ version: string }>('node_modules/@pnpm.e2e/bar/package.json').version).toBe('100.0.0')
  expect(loadJsonFileSync<{ version: string }>('node_modules/is-negative/package.json').version).toBe('1.0.0')
})

test('installing the same package with alias and no alias', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0', '@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0'],
    testDefaults({ nodeLinker: 'hoisted' })
  )

  expect(loadJsonFileSync<{ version: string }>('node_modules/@pnpm.e2e/pkg-with-1-aliased-dep/package.json').version).toBe('100.0.0')
  expect(loadJsonFileSync<{ version: string }>('node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json').version).toBe('100.0.0')
  expect(loadJsonFileSync<{ version: string }>('node_modules/dep/package.json').version).toBe('100.0.0')
})

test('run pre/postinstall scripts. bin files should be linked in a hoisted node_modules', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example'],
    testDefaults({
      fastUnpack: false,
      nodeLinker: 'hoisted',
      targetDependenciesField: 'devDependencies',
      allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
    })
  )

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-prepare.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()

  const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
  expect(typeof generatedByPreinstall).toBe('function')

  const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
  expect(typeof generatedByPostinstall).toBe('function')
})

// Covers https://github.com/pnpm/pnpm/issues/4209
test('running install scripts in a workspace that has no root project', async () => {
  prepareEmpty()

  await mutateModulesInSingleProject({
    manifest: {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
      },
    },
    mutation: 'install',
    rootDir: path.resolve('project-1') as ProjectRootDir,
  }, testDefaults({ fastUnpack: false, nodeLinker: 'hoisted', allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true } }))

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
})

test('hoistingLimits=dependencies should prevent packages from being hoisted past direct dependencies', async () => {
  prepareEmpty()

  await install({
    dependencies: {
      send: '0.17.2',
    },
  }, testDefaults({
    nodeLinker: 'hoisted',
    hoistingLimits: 'dependencies',
  }))

  expect(fs.existsSync('node_modules/ms')).toBeFalsy()
  expect(fs.existsSync('node_modules/send/node_modules/ms')).toBeTruthy()
})

test('externalDependencies should prevent package from being hoisted to the root', async () => {
  prepareEmpty()

  const externalDependencies = new Set(['ms'])
  await install({
    dependencies: {
      send: '0.17.2',
    },
  }, testDefaults({
    nodeLinker: 'hoisted',
    externalDependencies,
  }))

  expect(fs.existsSync('node_modules/ms')).toBeFalsy()
  expect(fs.existsSync('node_modules/send/node_modules/ms')).toBeTruthy()
})

test('linking bins of local projects when node-linker is set to hoisted', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',

    dependencies: {
      'project-2': 'workspace:*',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    bin: {
      'project-2': 'index.js',
    },
  }
  preparePackages([
    project1Manifest,
    project2Manifest,
  ])
  fs.writeFileSync('project-2/index.js', '#!/usr/bin/env node\nconsole.log("hello")', 'utf8')

  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects: [
      {
        buildIndex: 0,
        manifest: project1Manifest,
        rootDir: path.resolve('project-1') as ProjectRootDir,
      },
      {
        buildIndex: 1,
        manifest: project2Manifest,
        rootDir: path.resolve('project-2') as ProjectRootDir,
      },
    ],
    nodeLinker: 'hoisted',
  }))

  expect(fs.existsSync('project-1/node_modules/.bin/project-2')).toBeTruthy()
})

// https://github.com/pnpm/pnpm/issues/7568
test('bins of a nested package are removed when the package is deduped into the root node_modules', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '1.0.0',
    },
  }
  let project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '0.0.0',
    },
  }
  preparePackages([project1Manifest, project2Manifest])
  const installProjects = () => mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects: [
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
    ],
    nodeLinker: 'hoisted',
  }))

  await installProjects()
  expect(loadJsonFileSync<{ version: string }>('project-2/node_modules/@pnpm.e2e/hello-world-js-bin/package.json').version).toBe('0.0.0')
  expect(readBinEntries('project-2')).not.toHaveLength(0)

  project2Manifest = {
    ...project2Manifest,
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '1.0.0',
    },
  }
  await installProjects()

  expect(fs.existsSync('project-2/node_modules/@pnpm.e2e/hello-world-js-bin')).toBeFalsy()
  expect(readBinEntries('project-2')).toStrictEqual([])
  expect(readBinEntries('.')).not.toHaveLength(0)
})

test('a project dependency that is not hoisted goes in the project modules directory set by modulesDir', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '0.0.0',
    },
  }
  preparePackages([project1Manifest, project2Manifest])
  const installProjects = (opts: { force: boolean }) => mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects: [
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
    ],
    force: opts.force,
    modulesDir: 'vendor',
    nodeLinker: 'hoisted',
  }))

  await installProjects({ force: false })
  expect(loadJsonFileSync<{ version: string }>('vendor/@pnpm.e2e/hello-world-js-bin/package.json').version).toBe('1.0.0')
  expect(loadJsonFileSync<{ version: string }>('project-2/vendor/@pnpm.e2e/hello-world-js-bin/package.json').version).toBe('0.0.0')
  expect(fs.existsSync('project-2/node_modules')).toBeFalsy()
  expect(fs.readdirSync('project-2/vendor/.bin')).toContain('hello-world-js-bin')

  for (const modulesDir of ['vendor', 'project-2/vendor']) {
    fs.mkdirSync(path.join(modulesDir, 'stray'))
    fs.writeFileSync(path.join(modulesDir, 'stray/package.json'), JSON.stringify({ name: 'stray', version: '1.0.0' }))
  }
  await installProjects({ force: true })

  for (const modulesDir of ['vendor', 'project-2/vendor']) {
    expect(fs.existsSync(path.join(modulesDir, 'stray'))).toBeFalsy()
    expect(fs.existsSync(path.join(modulesDir, '.ignored/stray/package.json'))).toBeTruthy()
  }
})

test('an install removes a project dependency that an earlier layout left in the project node_modules', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '1.0.0',
    },
  }
  let project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '0.0.0',
    },
  }
  preparePackages([project1Manifest, project2Manifest])
  const installProjects = () => mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects: [
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
    ],
    modulesDir: 'vendor',
    nodeLinker: 'hoisted',
  }))
  await installProjects()

  // Only the recorded locations name the copy in node_modules.
  const legacyCopy = 'project-2/node_modules/@pnpm.e2e/hello-world-js-bin'
  fs.mkdirSync(path.dirname(legacyCopy), { recursive: true })
  fs.renameSync('project-2/vendor/@pnpm.e2e/hello-world-js-bin', legacyCopy)
  const modulesFile = loadJsonFileSync<{ hoistedLocations: Record<string, string[]> }>('vendor/.modules.yaml')
  const recordedCopy = path.join('project-2', 'vendor', '@pnpm.e2e', 'hello-world-js-bin')
  expect(Object.values(modulesFile.hoistedLocations).flat()).toContain(recordedCopy)
  for (const locations of Object.values(modulesFile.hoistedLocations)) {
    locations.forEach((location, index) => {
      if (location === recordedCopy) locations[index] = path.normalize(legacyCopy)
    })
  }
  modulesFile.hoistedLocations['not-a-package@1.0.0'] = ['', '.', 'project-1']
  fs.writeFileSync('vendor/.modules.yaml', JSON.stringify(modulesFile))

  project2Manifest = {
    ...project2Manifest,
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '~0.0.0',
    },
  }
  await installProjects()

  expect(fs.existsSync(legacyCopy)).toBeFalsy()
  expect(loadJsonFileSync<{ version: string }>('project-2/vendor/@pnpm.e2e/hello-world-js-bin/package.json').version).toBe('0.0.0')
  expect(fs.existsSync('project-1/package.json')).toBeTruthy()
  expect(fs.existsSync('project-2/package.json')).toBeTruthy()
})

test('a project dependency that is not hoisted stays in the project node_modules when modulesDir is absolute', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '0.0.0',
    },
  }
  preparePackages([project1Manifest, project2Manifest])
  const modulesDir = path.resolve('shared-modules')
  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects: [
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
    ],
    modulesDir,
    nodeLinker: 'hoisted',
  }))

  expect(loadJsonFileSync<{ version: string }>(path.join(modulesDir, '@pnpm.e2e/hello-world-js-bin/package.json')).version).toBe('1.0.0')
  expect(loadJsonFileSync<{ version: string }>('project-2/node_modules/@pnpm.e2e/hello-world-js-bin/package.json').version).toBe('0.0.0')
})

test('the orphan scan leaves alone a modules directory that is not inside its importer', async () => {
  const manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '1.0.0',
    },
  }
  preparePackages([manifest])
  fs.mkdirSync('stray')
  fs.writeFileSync('stray/package.json', JSON.stringify({ name: 'stray', version: '1.0.0' }))
  const installProject = (opts: { force: boolean }) => mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects: [
      {
        buildIndex: 0,
        manifest,
        rootDir: path.resolve('project-1') as ProjectRootDir,
      },
    ],
    force: opts.force,
    modulesDir: '.',
    nodeLinker: 'hoisted',
  }))

  await installProject({ force: false })
  await installProject({ force: true })

  expect(fs.existsSync('stray/package.json')).toBeTruthy()
  expect(fs.existsSync('project-1/package.json')).toBeTruthy()
})

test('the orphan scan leaves alone a modules directory that holds a project', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
  }
  preparePackages([
    project1Manifest,
    { location: 'packages/foo', package: project2Manifest },
  ])
  fs.mkdirSync('packages/stray')
  fs.writeFileSync('packages/stray/package.json', JSON.stringify({ name: 'stray', version: '1.0.0' }))
  const installProjects = (opts: { force: boolean }) => mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('packages/foo') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects: [
      {
        buildIndex: 0,
        manifest: project1Manifest,
        rootDir: path.resolve('project-1') as ProjectRootDir,
      },
      {
        buildIndex: 0,
        manifest: project2Manifest,
        rootDir: path.resolve('packages/foo') as ProjectRootDir,
      },
    ],
    force: opts.force,
    modulesDir: 'packages',
    nodeLinker: 'hoisted',
  }))

  await installProjects({ force: false })
  await installProjects({ force: true })

  expect(fs.existsSync('packages/foo/package.json')).toBeTruthy()
  expect(fs.existsSync('packages/stray/package.json')).toBeTruthy()
})

function readBinEntries (projectDir: string): string[] {
  return fs.readdirSync(path.join(projectDir, 'node_modules/.bin'))
    .filter((entry) => entry.startsWith('hello-world-js-bin'))
}

test('peerDependencies should be installed when autoInstallPeers is set to true and nodeLinker is set to hoisted', async () => {
  prepareEmpty()
  await install({
    dependencies: {
      'react-dom': '18.2.0',
    },
  }, testDefaults({
    nodeLinker: 'hoisted',
    autoInstallPeers: true,
  }))

  expect(fs.existsSync('node_modules/react')).toBeTruthy()
})

// Covers https://github.com/pnpm/pnpm/issues/8854
test('installing with hoisted node-linker a package that is a peer dependency of itself', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/peer-of-itself@1.0.0'],
    testDefaults({ nodeLinker: 'hoisted', save: true })
  )

  project.has('@pnpm.e2e/peer-of-itself')
  const lockfile = project.readLockfile()
  expect(lockfile.packages['@pnpm.e2e/peer-of-itself@1.0.0'].peerDependencies).toBeFalsy()
})

const testOnPosixAsNonRoot = process.platform === 'win32' || process.getuid?.() === 0 ? test.skip : test

// The next install trusts a recorded location, so an install that stops while
// replacing one must not leave it recorded.
testOnPosixAsNonRoot('an interrupted install forgets the hoisted locations it replaces', async () => {
  prepareEmpty()
  const manifest = (version: string) => ({
    dependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': version,
      'is-positive': '1.0.0',
    },
  })
  await install(manifest('100.0.0'), testDefaults({ nodeLinker: 'hoisted' }))
  fs.writeFileSync('node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/witness', '')

  // A read-only scope directory stops the linker before it replaces the package.
  fs.chmodSync('node_modules/@pnpm.e2e', 0o555)
  try {
    await expect(install(manifest('100.1.0'), testDefaults({ nodeLinker: 'hoisted' }))).rejects.toThrow()
  } finally {
    fs.chmodSync('node_modules/@pnpm.e2e', 0o755)
  }

  const { hoistedLocations } = loadJsonFileSync<{ hoistedLocations: Record<string, string[]> }>('node_modules/.modules.yaml')
  expect(hoistedLocations['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0']).toBeUndefined()
  expect(hoistedLocations['is-positive@1.0.0']).toStrictEqual([path.join('node_modules', 'is-positive')])

  await install(manifest('100.0.0'), testDefaults({ nodeLinker: 'hoisted' }))
  expect(fs.existsSync('node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/witness')).toBe(false)
  expect(loadJsonFileSync<{ version: string }>('node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json').version).toBe('100.0.0')
})
