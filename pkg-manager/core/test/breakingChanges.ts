import { promises as fs } from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type PnpmError } from '@pnpm/error'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from '@pnpm/core'
import rimraf from '@zkochan/rimraf'
import { isCI } from 'ci-info'
import { testDefaults } from './utils'

test('fail on non-compatible node_modules', async () => {
  prepareEmpty()
  const opts = await testDefaults()

  await saveModulesYaml('0.50.0', opts.storeDir)

  try {
    await addDependenciesToPackage({}, ['is-negative'], opts)
    throw new Error('should have failed')
  } catch (err: any) { // eslint-disable-line
    expect(err.code).toBe('ERR_PNPM_MODULES_BREAKING_CHANGE')
  }
})

test("don't fail on non-compatible node_modules when forced", async () => {
  prepareEmpty()
  const opts = await testDefaults({ force: true })

  await saveModulesYaml('0.50.0', opts.storeDir)

  await install({}, opts)
})

test("don't fail on non-compatible node_modules when forced in a workspace", async () => {
  preparePackages([
    {
      location: 'pkg',
      package: {},
    },
  ])
  const opts = await testDefaults({ force: true })

  process.chdir('pkg')
  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], await testDefaults({ lockfileDir: path.resolve('..') }))
  await rimraf('node_modules')

  process.chdir('..')

  await fs.writeFile('node_modules/.modules.yaml', `packageManager: pnpm@${3}\nstore: ${opts.storeDir}\nlayoutVersion: 1`)

  await install(manifest, { ...opts, dir: path.resolve('pkg'), lockfileDir: process.cwd() })
})

test('do not fail on non-compatible node_modules when forced with a named installation', async () => {
  prepareEmpty()
  const opts = await testDefaults()

  await saveModulesYaml('0.50.0', opts.storeDir)

  let err!: PnpmError
  try {
    await addDependenciesToPackage({}, ['is-negative'], opts)
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_MODULES_BREAKING_CHANGE')

  await install({}, {
    ...opts,
    confirmModulesPurge: false,
  })
})

test("don't fail on non-compatible store when forced", async () => {
  prepareEmpty()
  const opts = await testDefaults({ force: true })

  await saveModulesYaml('0.32.0', opts.storeDir)

  await install({}, opts)
})

test('do not fail on non-compatible store when forced during named installation', async () => {
  prepareEmpty()
  const opts = await testDefaults()

  await saveModulesYaml('0.32.0', opts.storeDir)

  let err!: PnpmError
  try {
    await addDependenciesToPackage({}, ['is-negative'], opts)
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_MODULES_BREAKING_CHANGE')

  await install({}, {
    ...opts,
    confirmModulesPurge: false,
  })
})

async function saveModulesYaml (pnpmVersion: string, storeDir: string) {
  await fs.mkdir('node_modules')
  await fs.writeFile('node_modules/.modules.yaml', `packageManager: pnpm@${pnpmVersion}\nstoreDir: ${storeDir}`)
}

test(`fail on non-compatible ${WANTED_LOCKFILE} when frozen lockfile installation is used`, async () => {
  if (isCI) {
    console.log('this test will always fail on CI servers')
    return
  }

  prepareEmpty()
  await fs.writeFile(WANTED_LOCKFILE, '')

  try {
    await addDependenciesToPackage({}, ['is-negative'], await testDefaults({ frozenLockfile: true }))
    throw new Error('should have failed')
  } catch (err: any) { // eslint-disable-line
    if (err.message === 'should have failed') throw err
    expect(err.code).toBe('ERR_PNPM_LOCKFILE_BREAKING_CHANGE')
  }
})

test(`don't fail on non-compatible ${WANTED_LOCKFILE} when forced`, async () => {
  prepareEmpty()
  await fs.writeFile(WANTED_LOCKFILE, '')

  await addDependenciesToPackage({}, ['is-negative'], await testDefaults({ force: true }))
})
