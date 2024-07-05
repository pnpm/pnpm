import fs from 'fs'
import path from 'path'
import { addDependenciesToPackage, install, mutateModules, mutateModulesInSingleProject } from '@pnpm/core'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { type ProjectRootDir } from '@pnpm/types'
import { sync as rimraf } from '@zkochan/rimraf'
import { sync as loadJsonFile } from 'load-json-file'
import { sync as readYamlFile } from 'read-yaml-file'
import symlinkDir from 'symlink-dir'
import { testDefaults } from '../utils'

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

  expect(readYamlFile<{ nodeLinker: string }>('node_modules/.modules.yaml').nodeLinker).toBe('hoisted')

  // If a package from node_modules is removed, it should be re-added.
  rimraf('node_modules/send')
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

test('overwriting (is-positive@3.0.0 with is-positive@latest)', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage(
    {},
    ['is-positive@3.0.0'],
    testDefaults({ nodeLinker: 'hoisted', save: true })
  )

  project.storeHas('is-positive', '3.0.0')

  const updatedManifest = await addDependenciesToPackage(
    manifest,
    ['is-positive@latest'],
    testDefaults({ nodeLinker: 'hoisted', save: true })
  )

  project.storeHas('is-positive', '3.1.0')
  expect(updatedManifest.dependencies?.['is-positive']).toBe('3.1.0')
  expect(loadJsonFile<{ version: string }>('node_modules/is-positive/package.json').version).toBe('3.1.0')
})

test('overwriting existing files in node_modules', async () => {
  prepareEmpty()
  await symlinkDir(__dirname, path.resolve('node_modules/is-positive'))

  const manifest = await addDependenciesToPackage(
    {},
    ['is-positive@3.0.0'],
    testDefaults({ nodeLinker: 'hoisted', save: true })
  )

  expect(manifest.dependencies?.['is-positive']).toBe('3.0.0')
  expect(loadJsonFile<{ version: string }>('node_modules/is-positive/package.json').version).toBe('3.0.0')
})

test('preserve subdeps on update', async () => {
  prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/foobarqar', version: '1.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/foobarqar@1.0.0', '@pnpm.e2e/bar@100.1.0'],
    testDefaults({ nodeLinker: 'hoisted' })
  )

  await addDependenciesToPackage(
    manifest,
    ['@pnpm.e2e/foobarqar@1.0.1'],
    testDefaults({ nodeLinker: 'hoisted' })
  )

  expect(loadJsonFile<{ version: string }>('node_modules/@pnpm.e2e/bar/package.json').version).toBe('100.1.0')
  expect(loadJsonFile<{ version: string }>('node_modules/@pnpm.e2e/foobarqar/package.json').version).toBe('1.0.1')
  expect(loadJsonFile<{ version: string }>('node_modules/@pnpm.e2e/foobarqar/node_modules/@pnpm.e2e/bar/package.json').version).toBe('100.0.0')
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
  manifest = await addDependenciesToPackage(
    manifest,
    ['is-negative@1.0.0'],
    testDefaults({ nodeLinker: 'hoisted', prefix: path.resolve('project-1'), targetDependenciesField: 'devDependencies' })
  )

  expect(manifest.dependencies).toStrictEqual({ '@pnpm.e2e/bar': '100.0.0' })
  expect(manifest.devDependencies).toStrictEqual({ 'is-negative': '1.0.0' })
  expect(loadJsonFile<{ version: string }>('node_modules/@pnpm.e2e/bar/package.json').version).toBe('100.0.0')
  expect(loadJsonFile<{ version: string }>('node_modules/is-negative/package.json').version).toBe('1.0.0')
})

test('installing the same package with alias and no alias', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0', '@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0'],
    testDefaults({ nodeLinker: 'hoisted' })
  )

  expect(loadJsonFile<{ version: string }>('node_modules/@pnpm.e2e/pkg-with-1-aliased-dep/package.json').version).toBe('100.0.0')
  expect(loadJsonFile<{ version: string }>('node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json').version).toBe('100.0.0')
  expect(loadJsonFile<{ version: string }>('node_modules/dep/package.json').version).toBe('100.0.0')
})

test('run pre/postinstall scripts. bin files should be linked in a hoisted node_modules', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({},
    ['@pnpm.e2e/pre-and-postinstall-scripts-example'],
    testDefaults({ fastUnpack: false, nodeLinker: 'hoisted', targetDependenciesField: 'devDependencies' })
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
  }, testDefaults({ fastUnpack: false, nodeLinker: 'hoisted' }))

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
})

test('hoistingLimits should prevent packages to be hoisted', async () => {
  prepareEmpty()

  const hoistingLimits = new Map()
  hoistingLimits.set('.@', new Set(['send']))
  await install({
    dependencies: {
      send: '0.17.2',
    },
  }, testDefaults({
    nodeLinker: 'hoisted',
    hoistingLimits,
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
