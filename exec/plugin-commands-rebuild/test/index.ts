/// <reference path="../../../__typings__/index.d.ts" />
import fs from 'fs'
import path from 'path'
import { readMsgpackFileSync, writeMsgpackFileSync } from '@pnpm/fs.msgpack-file'
import { getIndexFilePathInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'
import { ENGINE_NAME, STORE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { hashObject } from '@pnpm/crypto.object-hasher'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import { prepare } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import execa from 'execa'
import sinon from 'sinon'
import { DEFAULT_OPTS } from './utils/index.js'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')
const f = fixtures(import.meta.dirname)

test('rebuilds dependencies', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'add',
    '--save-dev',
    '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
    'pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf',
    '--config.enableGlobalVirtualStore=false',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    '--ignore-scripts',
    `--cache-dir=${cacheDir}`,
  ])

  let modules = project.readModulesManifest()
  expect(modules!.pendingBuilds).toStrictEqual([
    '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
    'test-git-fetch@https://codeload.github.com/pnpm/test-git-fetch/tar.gz/8b333f12d5357f4f25a654c305c826294cb073bf',
  ])

  const modulesManifest = project.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: false,
    registries: modulesManifest!.registries!,
    storeDir,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true, 'test-git-fetch': true },
  }, [])

  modules = project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.pendingBuilds).toHaveLength(0)

  {
    expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-prepare.js')).toBeFalsy()
    expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()

    const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }

  {
    const scripts = project.requireModule('test-git-fetch/output.json')
    expect(scripts).toStrictEqual([
      'preinstall',
      'install',
      'postinstall',
    ])
  }

  const cacheIntegrityPath = getIndexFilePathInCafs(path.join(storeDir, STORE_VERSION), getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0'), '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0')
  const cacheIntegrity = readMsgpackFileSync<PackageFilesIndex>(cacheIntegrityPath)!
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
  expect(cacheIntegrity.sideEffects!.get(sideEffectsKey)?.added?.has('generated-by-postinstall.js')).toBeTruthy()
  cacheIntegrity.sideEffects!.get(sideEffectsKey)!.added!.delete('generated-by-postinstall.js')
})

test('skipIfHasSideEffectsCache', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'add',
    '--save-dev',
    '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    '--ignore-scripts',
    `--cache-dir=${cacheDir}`,
    '--config.enableGlobalVirtualStore=false',
  ])

  const cacheIntegrityPath = getIndexFilePathInCafs(path.join(storeDir, STORE_VERSION), getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0'), '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0')
  let cacheIntegrity = readMsgpackFileSync<PackageFilesIndex>(cacheIntegrityPath)!
  const sideEffectsKey = `${ENGINE_NAME};deps=${hashObject({ '@pnpm.e2e/hello-world-js-bin@1.0.0': {} })}`
  cacheIntegrity.sideEffects = new Map([
    [sideEffectsKey, {
      added: new Map([
        ['foo', {
          digest: 'bar',
          mode: 1,
          size: 1,
        }],
      ]),
    }],
  ])
  writeMsgpackFileSync(cacheIntegrityPath, cacheIntegrity)

  let modules = project.readModulesManifest()
  expect(modules!.pendingBuilds).toStrictEqual([
    '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
  ])

  const modulesManifest = project.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: true,
    registries: modulesManifest!.registries!,
    skipIfHasSideEffectsCache: true,
    storeDir,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  }, [])

  modules = project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.pendingBuilds).toHaveLength(0)

  cacheIntegrity = readMsgpackFileSync<PackageFilesIndex>(cacheIntegrityPath)!
  expect(cacheIntegrity!.sideEffects).toBeTruthy()
  expect(cacheIntegrity!.sideEffects!.get(sideEffectsKey)?.added?.has('foo')).toBeTruthy()
})

test('rebuild does not fail when a linked package is present', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  f.copy('local-pkg', path.resolve('..', 'local-pkg'))

  await execa('node', [
    pnpmBin,
    'add',
    'link:../local-pkg',
    'is-positive',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--ignore-scripts',
    '--config.enableGlobalVirtualStore=false',
  ])

  const modulesManifest = project.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: false,
    registries: modulesManifest!.registries!,
    storeDir,
    allowBuilds: { 'local-pkg': true, 'is-positive': true },
  }, [])

  // see related issue https://github.com/pnpm/pnpm/issues/1155
})

test('rebuilds specific dependencies', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  await execa('node', [
    pnpmBin,
    'add',
    '--save-dev',
    '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
    'pnpm-e2e/install-scripts-example#b6cfdb8af6f8d5ebc5e7de6831af9d38084d765b',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--ignore-scripts',
    '--config.enableGlobalVirtualStore=false',
  ])

  const modulesManifest = project.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: false,
    registries: modulesManifest!.registries!,
    storeDir,
    allowBuilds: { 'install-scripts-example-for-pnpm': true },
  }, ['install-scripts-example-for-pnpm'])

  project.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
  project.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')

  const generatedByPreinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
  expect(typeof generatedByPreinstall).toBe('function')

  const generatedByPostinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
  expect(typeof generatedByPostinstall).toBe('function')
})

test('rebuild with pending option', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  await execa('node', [
    pnpmBin,
    'add',
    '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--ignore-scripts',
    '--config.enableGlobalVirtualStore=false',
  ])
  await execa('node', [
    pnpmBin,
    'add',
    'pnpm-e2e/install-scripts-example#b6cfdb8af6f8d5ebc5e7de6831af9d38084d765b',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--ignore-scripts',
    '--config.enableGlobalVirtualStore=false',
  ])

  let modules = project.readModulesManifest()
  expect(modules!.pendingBuilds).toHaveLength(2)
  expect(modules!.pendingBuilds[0]).toBe('@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0')
  // We are not doing an exact match here because when we hit rate limits, sometimes it gets resolved to
  // install-scripts-example-for-pnpm@git+https://github.com/pnpm-e2e/install-scripts-example.git#b6cfdb8af6f8d5ebc5e7de6831af9d38084d765b
  // not to
  // install-scripts-example-for-pnpm@https://codeload.github.com/pnpm-e2e/install-scripts-example/tar.gz/b6cfdb8af6f8d5ebc5e7de6831af9d38084d765b
  expect(modules!.pendingBuilds[1]).toMatch(/^install-scripts-example-for-pnpm@.*b6cfdb8af6f8d5ebc5e7de6831af9d38084d765b.*/)

  project.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
  project.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')

  project.hasNot('install-scripts-example-for-pnpm/generated-by-preinstall')
  project.hasNot('install-scripts-example-for-pnpm/generated-by-postinstall')

  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: true,
    registries: modules!.registries!,
    storeDir,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true, 'install-scripts-example-for-pnpm': true },
  }, [])

  modules = project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.pendingBuilds).toHaveLength(0)

  {
    const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }

  {
    const generatedByPreinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }
})

test('rebuild dependencies in correct order', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'add',
    '@pnpm.e2e/with-postinstall-a',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--ignore-scripts',
    '--config.enableGlobalVirtualStore=false',
  ])

  let modules = project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.pendingBuilds).not.toHaveLength(0)

  project.hasNot('.pnpm/@pnpm.e2e+with-postinstall-b@1.0.0/node_modules/@pnpm.e2e/with-postinstall-b/output.json')
  project.hasNot('@pnpm.e2e/with-postinstall-a/output.json')

  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: false,
    registries: modules!.registries!,
    storeDir,
    allowBuilds: { '@pnpm.e2e/with-postinstall-a': true, '@pnpm.e2e/with-postinstall-b': true },
  }, [])

  modules = project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.pendingBuilds).toHaveLength(0)

  expect(+project.requireModule('.pnpm/@pnpm.e2e+with-postinstall-b@1.0.0/node_modules/@pnpm.e2e/with-postinstall-b/output.json')[0] < +project.requireModule('@pnpm.e2e/with-postinstall-a/output.json')[0]).toBeTruthy()
})

test('rebuild links bins', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'add',
    '@pnpm.e2e/has-generated-bins-as-dep',
    '@pnpm.e2e/generated-bins',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--ignore-scripts',
    '--config.enableGlobalVirtualStore=false',
  ])

  expect(fs.existsSync(path.resolve('node_modules/.bin/cmd1'))).toBeFalsy()
  expect(fs.existsSync(path.resolve('node_modules/.bin/cmd2'))).toBeFalsy()

  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/has-generated-bins-as-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/has-generated-bins-as-dep/node_modules/.bin/cmd1'))).toBeFalsy()
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/has-generated-bins-as-dep/node_modules/.bin/cmd2'))).toBeFalsy()

  const modules = project.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: true,
    registries: modules!.registries!,
    storeDir,
    allowBuilds: { '@pnpm.e2e/has-generated-bins-as-dep': true, '@pnpm.e2e/generated-bins': true },
  }, [])

  project.isExecutable('.bin/cmd1')
  project.isExecutable('.bin/cmd2')
  project.isExecutable('@pnpm.e2e/has-generated-bins-as-dep/node_modules/.bin/cmd1')
  project.isExecutable('@pnpm.e2e/has-generated-bins-as-dep/node_modules/.bin/cmd2')
})

test(`rebuild should not fail on incomplete ${WANTED_LOCKFILE}`, async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
    optionalDependencies: {
      '@pnpm.e2e/not-compatible-with-any-os': '1.0.0',
    },
  })
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'install',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--ignore-scripts',
    '--config.enableGlobalVirtualStore=false',
  ])

  const reporter = sinon.spy()

  const modules = project.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: true,
    registries: modules!.registries!,
    reporter,
    storeDir,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true, '@pnpm.e2e/not-compatible-with-any-os': true },
  }, [])
})

test('rebuild with NODE_ENV=production should rebuild dev dependencies', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'add',
    '--save-dev',
    '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    '--ignore-scripts',
    `--cache-dir=${cacheDir}`,
    '--config.enableGlobalVirtualStore=false',
  ])

  process.env.NODE_ENV = 'production'

  try {
    await rebuild.handler({
      ...DEFAULT_OPTS,
      cacheDir,
      dir: process.cwd(),
      pending: true,
      registries: {
        default: REGISTRY,
      },
      storeDir,
      allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
    }, [])
  } finally {
    delete process.env.NODE_ENV
  }

  const modules = project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.pendingBuilds).toHaveLength(0)

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
})
