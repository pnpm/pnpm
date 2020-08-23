import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty } from '@pnpm/prepare'
import { copyFixture } from '@pnpm/test-fixtures'
import readYamlFile from 'read-yaml-file'
import { addDependenciesToPackage, mutateModules } from 'supi'
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import tape = require('tape')

const test = promisifyTape(tape)
const testSkip = promisifyTape(tape.skip)

testSkip('subsequent installation uses same lockfile directory by default', async (t: tape.Test) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], await testDefaults({ lockfileDir: path.resolve('..') }))

  await addDependenciesToPackage(manifest, ['is-negative@1.0.0'], await testDefaults())

  const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))

  t.deepEqual(Object.keys(lockfile.packages ?? {}), ['/is-negative/1.0.0', '/is-positive/1.0.0']) // eslint-disable-line @typescript-eslint/dot-notation
})

testSkip('subsequent installation fails if a different lockfile directory is specified', async (t: tape.Test) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], await testDefaults({ lockfileDir: path.resolve('..') }))

  let err!: Error & {code: string}

  try {
    await addDependenciesToPackage(manifest, ['is-negative@1.0.0'], await testDefaults({ lockfileDir: process.cwd() }))
  } catch (_) {
    err = _
  }

  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_LOCKFILE_DIRECTORY_MISMATCH', 'failed with correct error code')
})

test(`tarball location is correctly saved to ${WANTED_LOCKFILE} when a shared ${WANTED_LOCKFILE} is used`, async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await copyFixture('tar-pkg-with-dep-2/tar-pkg-with-dep-1.0.0.tgz', 'pkg.tgz')

  const lockfileDir = path.resolve('..')
  const [{ manifest }] = await mutateModules(
    [
      {
        allowNew: true,
        dependencySelectors: ['file:pkg.tgz'],
        manifest: {},
        mutation: 'installSome',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({ lockfileDir })
  )

  const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
  t.ok(lockfile.packages!['file:project/pkg.tgz'])
  t.equal(lockfile.packages!['file:project/pkg.tgz'].resolution['tarball'], 'file:project/pkg.tgz')

  await rimraf('node_modules')

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({ frozenLockfile: true, lockfileDir })
  )

  await project.has('tar-pkg-with-dep')
})
