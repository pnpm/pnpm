import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from 'supi'
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'
import path = require('path')
import exists = require('path-exists')
import tape = require('tape')

const test = promisifyTape(tape)

test('production install (with --production flag)', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      'pkg-with-1-dep': '100.0.0',
      'write-yaml': '1.0.0',
    },
    devDependencies: {
      '@zkochan/foo': '1.0.0',
      // js-yaml is also a dependency of write-yaml
      // covers issue https://github.com/pnpm/pnpm/issues/2882
      'js-yaml': '3.14.0',
      once: '^1.4.0',
    },
  }, await testDefaults({
    fastUnpack: false,
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: true,
    },
  }))

  t.notOk(await exists(path.resolve('node_modules/.pnpm/@zkochan/foo@1.0.0')))
  t.ok(await exists(path.resolve('node_modules/.pnpm/js-yaml@3.14.0')))
  await project.has('pkg-with-1-dep')
  await project.has('write-yaml')
  await project.hasNot('@zkochan/foo')
  await project.hasNot('js-yaml')
})

test('production install with --no-optional', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      'pkg-with-1-dep': '100.0.0',
      'write-yaml': '1.0.0',
    },
    optionalDependencies: {
      '@zkochan/foo': '1.0.0',
      // js-yaml is also a dependency of write-yaml
      // covers issue https://github.com/pnpm/pnpm/issues/2882
      'js-yaml': '3.14.0',
      once: '^1.4.0',
    },
  }, await testDefaults({
    fastUnpack: false,
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: false,
    },
  }))

  t.notOk(await exists(path.resolve('node_modules/.pnpm/@zkochan/foo@1.0.0')))
  t.ok(await exists(path.resolve('node_modules/.pnpm/js-yaml@3.14.0')))
  await project.has('pkg-with-1-dep')
  await project.has('write-yaml')
  await project.hasNot('@zkochan/foo')
  await project.hasNot('js-yaml')
})

test('install dev dependencies only', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await install({
    dependencies: {
      'is-positive': '1.0.0',
      once: '^1.4.0',
    },
    devDependencies: {
      inflight: '1.0.6',
    },
  }, await testDefaults({
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    },
  }))

  await project.has('inflight')
  await project.hasNot('once')

  {
    const lockfile = await project.readLockfile()
    t.ok(lockfile.packages['/is-positive/1.0.0'].dev === false)
  }

  {
    const currentLockfile = await project.readCurrentLockfile()
    t.notOk(currentLockfile.packages['/is-positive/1.0.0'], `prod dep only not added to current ${WANTED_LOCKFILE}`)
  }

  // Repeat normal installation adds missing deps to node_modules
  await install(manifest, await testDefaults())

  await project.has('once')

  {
    const currentLockfile = await project.readCurrentLockfile()
    t.ok(currentLockfile.packages['/is-positive/1.0.0'], `prod dep added to current ${WANTED_LOCKFILE}`)
  }
})

test('fail if installing different types of dependencies in a project that uses an external lockfile', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const lockfileDir = path.resolve('..')

  const manifest = await install({
    dependencies: {
      'is-positive': '1.0.0',
      once: '^1.4.0',
    },
    devDependencies: {
      inflight: '1.0.6',
    },
  }, await testDefaults({
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    },
    lockfileDir,
  }))

  await project.has('inflight')
  await project.hasNot('once')

  let err!: Error & { code: string }
  const newOpts = await testDefaults({
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir,
  })

  try {
    await addDependenciesToPackage(manifest, ['is-negative'], newOpts)
  } catch (_) {
    err = _
  }

  t.ok(err, 'installation failed')
  t.equal(err.code, 'ERR_PNPM_INCLUDED_DEPS_CONFLICT', 'error has correct error code')
  t.ok(err.message.includes('was installed with devDependencies. Current install wants optionalDependencies, dependencies, devDependencies.'), 'correct error message')

  await install(manifest, newOpts)
})
