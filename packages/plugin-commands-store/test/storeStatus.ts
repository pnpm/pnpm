import path from 'path'
import { PnpmError } from '@pnpm/error'
import { store } from '@pnpm/plugin-commands-store'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import tempy from 'tempy'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.cjs')

test('CLI fails when store status finds modified packages', async () => {
  const project = prepare()
  const tmp = tempy.directory()
  const cacheDir = path.join(tmp, 'cache')
  const storeDir = path.join(tmp, 'store')

  await execa('node', [
    pnpmBin,
    'add',
    'is-positive@3.1.0',
    `--store-dir=${storeDir}`,
    `--registry=${REGISTRY}`,
    '--verify-store-integrity',
  ])

  await rimraf('node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive/index.js')

  let err!: PnpmError
  const modulesState = await project.readModulesManifest()
  try {
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: REGISTRY,
      },
      registries: modulesState!.registries!,
      storeDir,
      userConfig: {},
    }, ['status'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_MODIFIED_DEPENDENCY')
  expect(err['modified'].length).toBe(1)
  expect(err['modified'][0]).toMatch(/is-positive/)
})

test('CLI does not fail when store status does not find modified packages', async () => {
  const project = prepare()
  const tmp = tempy.directory()
  const cacheDir = path.join(tmp, 'cache')
  const storeDir = path.join(tmp, 'store')

  await execa('node', [
    pnpmBin,
    `--store-dir=${storeDir}`,
    `--registry=${REGISTRY}`,
    '--verify-store-integrity',
    'add',
    'eslint@3.4.0',
    'gulp@4.0.2',
    'highcharts@5.0.10',
    'is-positive@3.1.0',
    'react@15.4.1',
    'webpack@5.24.2',
    'koorchik/node-mole-rpc',
  ])
  // store status does not fail on not installed optional dependencies
  await execa('node', [
    pnpmBin,
    'add',
    'not-compatible-with-any-os',
    '--save-optional',
    `--store-dir=${storeDir}`,
    `--registry=${REGISTRY}`,
    '--verify-store-integrity',
  ])

  const modulesState = await project.readModulesManifest()
  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: modulesState!.registries!,
    storeDir,
    userConfig: {},
  }, ['status'])
})
