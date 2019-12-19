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

  await execa('pnpm', ['add', 'is-positive@3.1.0', '--store-dir', storeDir, '--registry', REGISTRY])

  const isPositive = await project.resolve('is-positive', '3.1.0', 'index.js')
  await rimraf(isPositive)

  try {
    await store.handler(['status'], {
      dir: process.cwd(),
      lock: true,
      rawConfig: {
        registry: REGISTRY,
      },
      registries: { default: REGISTRY },
      storeDir,
    })
    t.fail('CLI should have failed')
  } catch (err) {
    t.pass('CLI failed')
  }
  t.end()
})

test('CLI does not fail when store status does not find modified packages', async function (t) {
  const project = prepare(t)
  const storeDir = path.resolve('pnpm-store')

  await execa('pnpm', ['add', 'is-positive@3.1.0', '--store-dir', storeDir, '--registry', REGISTRY])

  await store.handler(['status'], {
    dir: process.cwd(),
    lock: true,
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir,
  })
  t.pass('CLI did not fail')
  t.end()
})
