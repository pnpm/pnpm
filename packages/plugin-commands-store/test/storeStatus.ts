import PnpmError from '@pnpm/error'
import { store } from '@pnpm/plugin-commands-store'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import rimraf = require('@zkochan/rimraf')
import execa = require('execa')
import path = require('path')
import test = require('tape')

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`

test('CLI fails when store status finds modified packages', async function (t) {
  const project = prepare(t)
  const storeDir = path.resolve('pnpm-store')

  await execa('pnpm', ['add', 'is-positive@3.1.0', '--store-dir', storeDir, '--registry', REGISTRY, '--verify-store-integrity'])

  const isPositive = await project.resolve('is-positive', '3.1.0', 'index.js')
  await rimraf(isPositive)

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

test('CLI does not fail when store status does not find modified packages', async function (t) {
  const project = prepare(t)
  const storeDir = path.resolve('pnpm-store')

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
