import assertStore from '@pnpm/assert-store'
import { store } from '@pnpm/plugin-commands-store'
import prepare, { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { stripIndent } from 'common-tags'
import execa = require('execa')
import path = require('path')
import test = require('tape')

const STORE_VERSION = 'v3'
const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const DEFAULT_OPTS = {
  lock: false,
  rawConfig: {
    registry: REGISTRY,
  },
  registries: { default: REGISTRY },
}

test('find usages for single package in store and in a project', async (t) => {
  const project = prepare(t)
  const storeDir = path.resolve('store')

  // Install deps
  await execa('pnpm', ['add', 'is-negative@2.1.0', 'is-odd@3.0.0', '--store-dir', storeDir, '--registry', REGISTRY])
  await project.storeHas('is-negative', '2.1.0')

  {
    const output = await store.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      storeDir,
    }, ['usages', 'is-negative'])
    t.equal(
      output,
      stripIndent`
        Package: is-negative
        └─┬ Package in store: localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0
          └── Project with dependency: ${path.resolve('node_modules')}` + '\n',
      'finds usages by package name',
    )
  }
  {
    const output = await store.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      storeDir,
    }, ['usages', 'is-negative@2.1.0'])
    t.equal(
      output,
      stripIndent`
        Package: is-negative@2.1.0
        └─┬ Package in store: localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0
          └── Project with dependency: ${path.resolve('node_modules')}` + '\n',
      'finds usages by package name and version',
    )
  }
  {
    await project.storeHasNot('should-not-exist-uhsalzkj')
    const output = await store.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      storeDir,
    }, ['usages', 'should-not-exist-uhsalzkj'])
    t.equal(
      output,
      stripIndent`
        Package: should-not-exist-uhsalzkj
        └── Not found in store` + '\n',
      'cannot find usages if package does not exist',
    )
  }
  {
    const output = await store.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      storeDir,
    }, ['usages', 'is-negative', 'is-odd'])
    t.equal(
      output,
      stripIndent`
        Package: is-negative
        └─┬ Package in store: localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0
          └── Project with dependency: ${path.resolve('node_modules')}

        Package: is-odd
        └─┬ Package in store: localhost+${REGISTRY_MOCK_PORT}/is-odd/3.0.0
          └── Project with dependency: ${path.resolve('node_modules')}` + '\n',
      'finds usages of two packages',
    )
  }

  t.end()
})

test('find usages for package(s) in store but not in any projects', async (t) => {
  prepareEmpty(t)
  const storeDir = path.resolve('store')
  const { storeHas } = assertStore(t, path.join(storeDir, STORE_VERSION))

  // Add dependency directly to store (not to the project)
  await store.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    storeDir,
  }, ['add', 'is-negative@2.1.0'])
  await storeHas('is-negative', '2.1.0')

  {
    const output = await store.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      storeDir,
    }, ['usages', 'is-negative'])
    t.equal(
      output,
      stripIndent`
        Package: is-negative
        └─┬ Package in store: localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0
          └── No pnpm projects using this package` + '\n',
      'finds usage of package',
    )
  }
  await store.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    storeDir,
  }, ['add', 'is-negative@2.0.0'])
  await storeHas('is-negative', '2.0.0')
  {
    const output = await store.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      storeDir,
    }, ['usages', 'is-negative'])
    t.equal(
      output,
      stripIndent`
        Package: is-negative
        ├─┬ Package in store: localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0
        │ └── No pnpm projects using this package
        └─┬ Package in store: localhost+${REGISTRY_MOCK_PORT}/is-negative/2.0.0
          └── No pnpm projects using this package` + '\n',
      'finds usages of packages',
    )
  }

  t.end()
})
