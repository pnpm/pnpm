import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { sync as readYamlFile } from 'read-yaml-file'
import { addDependenciesToPackage, mutateModulesInSingleProject } from '@pnpm/core'
import { type ProjectRootDir, type DepPath } from '@pnpm/types'
import { sync as rimraf } from '@zkochan/rimraf'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test.skip('subsequent installation uses same lockfile directory by default', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], testDefaults({ lockfileDir: path.resolve('..') }))

  await addDependenciesToPackage(manifest, ['is-negative@1.0.0'], testDefaults())

  const lockfile = readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))

  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['is-negative/1.0.0', 'is-positive/1.0.0'])
})

test.skip('subsequent installation fails if a different lockfile directory is specified', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], testDefaults({ lockfileDir: path.resolve('..') }))

  let err!: Error & { code: string }

  try {
    await addDependenciesToPackage(manifest, ['is-negative@1.0.0'], testDefaults({ lockfileDir: process.cwd() }))
    throw new Error('test failed')
  } catch (_: any) { // eslint-disable-line
    err = _
  }

  expect(err.code).toBe('ERR_PNPM_LOCKFILE_DIRECTORY_MISMATCH')
})

test(`tarball location is correctly saved to ${WANTED_LOCKFILE} when a shared ${WANTED_LOCKFILE} is used`, async () => {
  const project = prepareEmpty()

  f.copy('tar-pkg-with-dep-2/tar-pkg-with-dep-1.0.0.tgz', 'pkg.tgz')

  const lockfileDir = path.resolve('..')
  const { manifest } = await mutateModulesInSingleProject({
    allowNew: true,
    dependencySelectors: ['file:pkg.tgz'],
    manifest: {},
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ lockfileDir }))

  const lockfile = readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
  expect(lockfile.packages!['tar-pkg-with-dep@file:project/pkg.tgz' as DepPath]).toBeTruthy()
  expect(lockfile.packages!['tar-pkg-with-dep@file:project/pkg.tgz' as DepPath].resolution).toHaveProperty(['tarball'], 'file:project/pkg.tgz')

  rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true, lockfileDir }))

  project.has('tar-pkg-with-dep')
})
