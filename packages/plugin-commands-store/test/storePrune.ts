import assertStore from '@pnpm/assert-store'
import { Lockfile } from '@pnpm/lockfile-file'
import { store } from '@pnpm/plugin-commands-store'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import fs = require('fs')
import rimraf = require('@zkochan/rimraf')
import execa = require('execa')
import path = require('path')
import R = require('ramda')
import sinon = require('sinon')
import ssri = require('ssri')
import test = require('tape')

const STORE_VERSION = 'v3'
const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.js')

test('remove unreferenced packages', async (t) => {
  const project = prepare(t)
  const storeDir = path.resolve('store')

  await execa('node', [pnpmBin, 'add', 'is-negative@2.1.0', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'remove', 'is-negative', '--store-dir', storeDir], { env: { npm_config_registry: REGISTRY } })

  await project.storeHas('is-negative', '2.1.0')

  const reporter = sinon.spy()
  await store.handler({
    dir: process.cwd(),
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
  }, ['prune'])

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: 'Removed 1 package',
  }), 'report removal')

  await project.storeHasNot('is-negative', '2.1.0')

  reporter.resetHistory()
  await store.handler({
    dir: process.cwd(),
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
  }, ['prune'])

  t.notOk(reporter.calledWithMatch({
    level: 'info',
    message: 'Removed 1 package',
  }))
  t.end()
})

test.skip('remove packages that are used by project that no longer exist', async (t) => {
  prepare(t)
  const storeDir = path.resolve('store', STORE_VERSION)
  const { cafsHas, cafsHasNot } = assertStore(t, storeDir)

  await execa('node', [pnpmBin, 'add', 'is-negative@2.1.0', '--store-dir', storeDir, '--registry', REGISTRY])

  await rimraf('node_modules')

  await cafsHas(ssri.fromHex('f0d86377aa15a64c34961f38ac2a9be2b40a1187', 'sha1').toString())

  const reporter = sinon.spy()
  await store.handler({
    dir: process.cwd(),
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
  }, ['prune'])

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: `- localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`,
  }))

  await cafsHasNot(ssri.fromHex('f0d86377aa15a64c34961f38ac2a9be2b40a1187', 'sha1').toString())
  t.end()
})

test('keep dependencies used by others', async (t) => {
  const project = prepare(t)
  const storeDir = path.resolve('store')
  await execa('node', [pnpmBin, 'add', 'camelcase-keys@3.0.0', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'add', 'hastscript@3.0.0', '--save-dev', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'remove', 'camelcase-keys', '--store-dir', storeDir], { env: { npm_config_registry: REGISTRY } })

  await project.storeHas('camelcase-keys', '3.0.0')
  await project.hasNot('camelcase-keys')

  await project.storeHas('camelcase', '3.0.0')

  await project.storeHas('map-obj', '1.0.1')
  await project.hasNot('map-obj')

  // all dependencies are marked as dev
  const lockfile = await project.readLockfile() as Lockfile
  t.notOk(R.isEmpty(lockfile.packages))

  Object.entries(lockfile.packages ?? {}).forEach(([depPath, dep]) => t.ok(dep.dev, `${depPath} is dev`))

  await store.handler({
    dir: process.cwd(),
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir,
  }, ['prune'])

  await project.storeHasNot('camelcase-keys', '3.0.0')
  await project.storeHasNot('map-obj', '1.0.1')
  await project.storeHas('camelcase', '3.0.0')
  t.end()
})

test('keep dependency used by package', async (t) => {
  const project = prepare(t)
  const storeDir = path.resolve('store')
  await execa('node', [pnpmBin, 'add', 'is-not-positive@1.0.0', 'is-positive@3.1.0', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'remove', 'is-not-positive', '--store-dir', storeDir], { env: { npm_config_registry: REGISTRY } })

  await store.handler({
    dir: process.cwd(),
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir,
  }, ['prune'])

  await project.storeHas('is-positive', '3.1.0')
  t.end()
})

test('prune will skip scanning non-directory in storeDir', async (t) => {
  prepare(t)
  const storeDir = path.resolve('store')
  await execa('node', [pnpmBin, 'add', 'is-not-positive@1.0.0', 'is-positive@3.1.0', '--store-dir', storeDir, '--registry', REGISTRY])
  fs.writeFileSync(path.join(storeDir, STORE_VERSION, 'files/.DS_store'), 'foobar')

  await store.handler({
    dir: process.cwd(),
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir,
  }, ['prune'])

  t.end()
})
