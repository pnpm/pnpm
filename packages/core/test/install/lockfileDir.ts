import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty } from '@pnpm/prepare'
import fixtures from '@pnpm/test-fixtures'
import readYamlFile from 'read-yaml-file'
import { addDependenciesToPackage, mutateModulesInSingleProject } from '@pnpm/core'
import rimraf from '@zkochan/rimraf'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test.skip('subsequent installation uses same lockfile directory by default', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], await testDefaults({ lockfileDir: path.resolve('..') }))

  await addDependenciesToPackage(manifest, ['is-negative@1.0.0'], await testDefaults())

  const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))

  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['/is-negative/1.0.0', '/is-positive/1.0.0'])
})

test.skip('subsequent installation fails if a different lockfile directory is specified', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], await testDefaults({ lockfileDir: path.resolve('..') }))

  let err!: Error & { code: string }

  try {
    await addDependenciesToPackage(manifest, ['is-negative@1.0.0'], await testDefaults({ lockfileDir: process.cwd() }))
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
    rootDir: process.cwd(),
  }, await testDefaults({ lockfileDir }))

  const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
  expect(lockfile.packages!['file:project/pkg.tgz']).toBeTruthy()
  expect(lockfile.packages!['file:project/pkg.tgz'].resolution['tarball']).toBe('file:project/pkg.tgz')

  await rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ frozenLockfile: true, lockfileDir }))

  await project.has('tar-pkg-with-dep')
})
