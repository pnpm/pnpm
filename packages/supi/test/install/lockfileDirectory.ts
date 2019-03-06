import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-file'
import prepare from '@pnpm/prepare'
import ncpCB = require('ncp')
import path = require('path')
import readYamlFile from 'read-yaml-file'
import rimraf = require('rimraf-then')
import { addDependenciesToPackage, mutateModules, rebuild } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import promisify = require('util.promisify')
import { pathToLocalPkg, testDefaults } from '../utils'

const ncp = promisify(ncpCB)
const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)
const testSkip = promisifyTape(tape.skip)

testSkip('subsequent installation uses same lockfile directory by default', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['is-positive@1.0.0'], await testDefaults({ lockfileDirectory: path.resolve('..') }))

  await addDependenciesToPackage(['is-negative@1.0.0'], await testDefaults())

  const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))

  t.deepEqual(Object.keys(lockfile['packages'] || {}), ['/is-negative/1.0.0', '/is-positive/1.0.0']) // tslint:disable-line:no-string-literal
})

testSkip('subsequent installation fails if a different lockfile directory is specified', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['is-positive@1.0.0'], await testDefaults({ lockfileDirectory: path.resolve('..') }))

  let err!: Error & {code: string}

  try {
    await addDependenciesToPackage(['is-negative@1.0.0'], await testDefaults({ lockfileDirectory: process.cwd() }))
  } catch (_) {
    err = _
  }

  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_LOCKFILE_DIRECTORY_MISMATCH', 'failed with correct error code')
})

test(`tarball location is correctly saved to ${WANTED_LOCKFILE} when a shared ${WANTED_LOCKFILE} is used`, async (t: tape.Test) => {
  const project = prepare(t)

  await ncp(path.join(pathToLocalPkg('tar-pkg-with-dep-2'), 'tar-pkg-with-dep-1.0.0.tgz'), 'pkg.tgz')

  const lockfileDirectory = path.resolve('..')
  await mutateModules(
    [
      {
        allowNew: true,
        dependencySelectors: ['file:pkg.tgz'],
        mutation: 'installSome',
        prefix: process.cwd(),
      },
    ],
    await testDefaults({ lockfileDirectory }),
  )

  const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
  t.ok(lockfile.packages!['file:project/pkg.tgz'])
  t.equal(lockfile.packages!['file:project/pkg.tgz'].resolution['tarball'], 'file:project/pkg.tgz')

  await rimraf('node_modules')

  await mutateModules(
    [
      {
        buildIndex: 0,
        mutation: 'install',
        prefix: process.cwd(),
      }
    ],
    await testDefaults({ frozenLockfile: true, lockfileDirectory }),
  )

  await project.has('tar-pkg-with-dep')

  await rebuild([{ buildIndex: 0, prefix: process.cwd() }], await testDefaults({ lockfileDirectory }))

  t.pass('rebuild did not fail')
})
