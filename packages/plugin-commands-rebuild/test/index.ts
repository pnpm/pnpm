/// <reference path="../../../typings/index.d.ts" />
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import prepare, { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import fixtures from '@pnpm/test-fixtures'
import execa from 'execa'
import exists from 'path-exists'
import sinon from 'sinon'
import { DEFAULT_OPTS } from './utils'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.cjs')
const f = fixtures(__dirname)

test('rebuilds dependencies', async () => {
  const project = prepareEmpty()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'add',
    '--save-dev',
    '@pnpm.e2e/pre-and-postinstall-scripts-example',
    'pnpm/test-git-fetch#299c6d89507571462b992b92407a8a07663e32ee',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    '--ignore-scripts',
    `--cache-dir=${cacheDir}`,
  ])

  let modules = await project.readModulesManifest()
  expect(modules!.pendingBuilds).toStrictEqual([
    '/@pnpm.e2e/pre-and-postinstall-scripts-example/1.0.0',
    'github.com/pnpm/test-git-fetch/299c6d89507571462b992b92407a8a07663e32ee',
  ])

  const modulesManifest = await project.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: false,
    registries: modulesManifest!.registries!,
    storeDir,
  }, [])

  modules = await project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.pendingBuilds.length).toBe(0)

  {
    expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-prepare.js')).toBeFalsy()
    expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()

    const generatedByPreinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function')

    const generatedByPostinstall = project.requireModule('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function')
  }

  {
    const scripts = project.requireModule('test-git-fetch/output.json')
    expect(scripts[0]).toBe('preinstall')
    expect(scripts[1]).toBe('install')
    expect(scripts[2]).toBe('postinstall')
    expect(scripts[3]).toBe('prepare')
  }
})

test('rebuild does not fail when a linked package is present', async () => {
  const project = prepareEmpty()
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
  ])

  const modulesManifest = await project.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: false,
    registries: modulesManifest!.registries!,
    storeDir,
  }, [])

  // see related issue https://github.com/pnpm/pnpm/issues/1155
})

test('rebuilds specific dependencies', async () => {
  const project = prepareEmpty()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  await execa('node', [
    pnpmBin,
    'add',
    '--save-dev',
    '@pnpm.e2e/pre-and-postinstall-scripts-example',
    'pnpm-e2e/install-scripts-example#b6cfdb8af6f8d5ebc5e7de6831af9d38084d765b',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--ignore-scripts',
  ])

  const modulesManifest = await project.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: false,
    registries: modulesManifest!.registries!,
    storeDir,
  }, ['install-scripts-example-for-pnpm'])

  await project.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
  await project.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')

  const generatedByPreinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
  expect(typeof generatedByPreinstall).toBe('function')

  const generatedByPostinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
  expect(typeof generatedByPostinstall).toBe('function')
})

test('rebuild with pending option', async () => {
  const project = prepareEmpty()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  await execa('node', [
    pnpmBin,
    'add',
    '@pnpm.e2e/pre-and-postinstall-scripts-example',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--ignore-scripts',
  ])
  await execa('node', [
    pnpmBin,
    'add',
    'pnpm-e2e/install-scripts-example#b6cfdb8af6f8d5ebc5e7de6831af9d38084d765b',
    `--registry=${REGISTRY}`,
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--ignore-scripts',
  ])

  let modules = await project.readModulesManifest()
  expect(modules!.pendingBuilds).toStrictEqual([
    '/@pnpm.e2e/pre-and-postinstall-scripts-example/1.0.0',
    'github.com/pnpm-e2e/install-scripts-example/b6cfdb8af6f8d5ebc5e7de6831af9d38084d765b',
  ])

  await project.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall')
  await project.hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall')

  await project.hasNot('install-scripts-example-for-pnpm/generated-by-preinstall')
  await project.hasNot('install-scripts-example-for-pnpm/generated-by-postinstall')

  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: true,
    registries: modules!.registries!,
    storeDir,
  }, [])

  modules = await project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.pendingBuilds.length).toBe(0)

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
  const project = prepareEmpty()
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
  ])

  let modules = await project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.pendingBuilds.length).not.toBe(0)

  await project.hasNot('.pnpm/@pnpm.e2e+with-postinstall-b@1.0.0/node_modules/@pnpm.e2e/with-postinstall-b/output.json')
  await project.hasNot('@pnpm.e2e/with-postinstall-a/output.json')

  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: false,
    registries: modules!.registries!,
    storeDir,
  }, [])

  modules = await project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.pendingBuilds.length).toBe(0)

  expect(+project.requireModule('.pnpm/@pnpm.e2e+with-postinstall-b@1.0.0/node_modules/@pnpm.e2e/with-postinstall-b/output.json')[0] < +project.requireModule('@pnpm.e2e/with-postinstall-a/output.json')[0]).toBeTruthy()
})

test('rebuild links bins', async () => {
  const project = prepareEmpty()
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
  ])

  expect(await exists(path.resolve('node_modules/.bin/cmd1'))).toBeFalsy()
  expect(await exists(path.resolve('node_modules/.bin/cmd2'))).toBeFalsy()

  expect(await exists(path.resolve('node_modules/@pnpm.e2e/has-generated-bins-as-dep/package.json'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/@pnpm.e2e/has-generated-bins-as-dep/node_modules/.bin/cmd1'))).toBeFalsy()
  expect(await exists(path.resolve('node_modules/@pnpm.e2e/has-generated-bins-as-dep/node_modules/.bin/cmd2'))).toBeFalsy()

  const modules = await project.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: true,
    registries: modules!.registries!,
    storeDir,
  }, [])

  await project.isExecutable('.bin/cmd1')
  await project.isExecutable('.bin/cmd2')
  await project.isExecutable('@pnpm.e2e/has-generated-bins-as-dep/node_modules/.bin/cmd1')
  await project.isExecutable('@pnpm.e2e/has-generated-bins-as-dep/node_modules/.bin/cmd2')
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
  ])

  const reporter = sinon.spy()

  const modules = await project.readModulesManifest()
  await rebuild.handler({
    ...DEFAULT_OPTS,
    cacheDir,
    dir: process.cwd(),
    pending: true,
    registries: modules!.registries!,
    reporter,
    storeDir,
  }, [])
})
