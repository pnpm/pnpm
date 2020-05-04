import PnpmError from '@pnpm/error'
import { store } from '@pnpm/plugin-commands-store'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import rimraf = require('@zkochan/rimraf')
import execa = require('execa')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`

// TODO: unskip when alpha.4 is out
test.skip('CLI fails when store status finds modified packages', async function (t) {
  const project = prepare(t)
  const storeDir = tempy.directory()

  await execa('pnpm', ['add', 'is-positive@3.1.0', '--store-dir', storeDir, '--registry', REGISTRY, '--verify-store-integrity'])

  await rimraf(`node_modules/.pnpm/localhost+${REGISTRY_MOCK_PORT}/is-positive/3.1.0/node_modules/is-positive/index.js`)

  let err!: PnpmError
  try {
    await store.handler({
      dir: process.cwd(),
      lock: false,
      rawConfig: {
        registry: REGISTRY,
      },
      registries: { default: REGISTRY },
      storeDir,
    }, ['status'])
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_MODIFIED_DEPENDENCY')
  t.equal(err['modified'].length, 1)
  t.ok(err['modified'][0].includes('is-positive'))
  t.end()
})

// TODO: unskip when alpha.4 is out
test.skip('CLI does not fail when store status does not find modified packages', async function (t) {
  const project = prepare(t)
  const storeDir = tempy.directory()

  await execa('pnpm', ['add', 'is-positive@3.1.0', '--store-dir', storeDir, '--registry', REGISTRY, '--verify-store-integrity'])
  // store status does not fail on not installed optional dependencies
  await execa('pnpm', ['add', 'not-compatible-with-any-os', '--save-optional', '--store-dir', storeDir, '--registry', REGISTRY, '--verify-store-integrity'])

  await store.handler({
    dir: process.cwd(),
    lock: false,
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir,
  }, ['status'])
  t.pass('CLI did not fail')
  t.end()
})
