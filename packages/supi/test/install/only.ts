import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from 'supi'
import exists from 'path-exists'
import { testDefaults } from '../utils'

test('production install (with --production flag)', async () => {
  const project = prepareEmpty()

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

  expect(await exists(path.resolve('node_modules/.pnpm/@zkochan/foo@1.0.0'))).toBeFalsy()
  expect(await exists(path.resolve('node_modules/.pnpm/js-yaml@3.14.0'))).toBeTruthy()
  await project.has('pkg-with-1-dep')
  await project.has('write-yaml')
  await project.hasNot('@zkochan/foo')
  await project.hasNot('js-yaml')
})

test('production install with --no-optional', async () => {
  const project = prepareEmpty()

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

  expect(await exists(path.resolve('node_modules/.pnpm/@zkochan/foo@1.0.0'))).toBeFalsy()
  expect(await exists(path.resolve('node_modules/.pnpm/js-yaml@3.14.0'))).toBeTruthy()
  await project.has('pkg-with-1-dep')
  await project.has('write-yaml')
  await project.hasNot('@zkochan/foo')
  await project.hasNot('js-yaml')
})

test('install dev dependencies only', async () => {
  const project = prepareEmpty()

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
    expect(lockfile.packages['/is-positive/1.0.0'].dev === false).toBeTruthy()
  }

  {
    const currentLockfile = await project.readCurrentLockfile()
    expect(currentLockfile.packages['/is-positive/1.0.0']).toBeFalsy()
  }

  // Repeat normal installation adds missing deps to node_modules
  await install(manifest, await testDefaults())

  await project.has('once')

  {
    const currentLockfile = await project.readCurrentLockfile()
    expect(currentLockfile.packages['/is-positive/1.0.0']).toBeTruthy()
  }
})

test('fail if installing different types of dependencies in a project that uses an external lockfile', async () => {
  const project = prepareEmpty()

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

  const newOpts = await testDefaults({
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir,
  })

  await expect(
    addDependenciesToPackage(manifest, ['is-negative'], newOpts)
  ).rejects.toThrow(/was installed with devDependencies. Current install wants optionalDependencies, dependencies, devDependencies./)

  await install(manifest, newOpts)
})
