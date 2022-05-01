import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
} from '@pnpm/core'
import { addDistTag } from '@pnpm/registry-mock'
import { testDefaults } from '../utils'

test('save to package.json (is-positive@^1.0.0)', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['is-positive@^1.0.0'], await testDefaults({ save: true }))

  await project.has('is-positive')

  expect(manifest.dependencies).toStrictEqual({ 'is-positive': '^1.0.0' })
})

// NOTE: this works differently for global installations. See similar tests in global.ts
test("don't override existing spec in package.json on named installation", async () => {
  const project = prepareEmpty()
  let manifest = await addDependenciesToPackage({
    dependencies: {
      'is-negative': '^1.0.0', // this will be updated
      'is-positive': '^2.0.0', // this will be kept as no newer version is available from the range
      sec: 'sindresorhus/sec#main',
    },
  }, ['is-positive'], await testDefaults())
  manifest = await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults())
  manifest = await addDependenciesToPackage(manifest, ['sec'], await testDefaults())

  expect(project.requireModule('is-positive/package.json').version).toBe('2.0.0')
  expect(project.requireModule('is-negative/package.json').version).toBe('1.0.1')

  expect(manifest.dependencies).toStrictEqual({
    'is-negative': '^1.0.1',
    'is-positive': '^2.0.0',
    sec: 'github:sindresorhus/sec#main',
  })
})

test('saveDev scoped module to package.json (@rstacruz/tap-spec)', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['@rstacruz/tap-spec'], await testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies' }))

  const m = project.requireModule('@rstacruz/tap-spec')
  expect(typeof m).toBe('function')

  expect(manifest.devDependencies).toStrictEqual({ '@rstacruz/tap-spec': '^4.1.1' })
})

test('dependency should not be added to package.json if it is already there', async () => {
  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })

  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({
    devDependencies: {
      foo: '^100.0.0',
    },
    optionalDependencies: {
      bar: '^100.0.0',
    },
  }, ['foo', 'bar'], await testDefaults())

  expect(manifest).toStrictEqual({
    devDependencies: {
      foo: '^100.0.0',
    },
    optionalDependencies: {
      bar: '^100.0.0',
    },
  })

  const lockfile = await project.readLockfile()

  expect(lockfile.devDependencies.foo).toBe('100.0.0')
  expect(lockfile.packages['/foo/100.0.0'].dev).toBeTruthy()

  expect(lockfile.optionalDependencies.bar).toBe('100.0.0')
  expect(lockfile.packages['/bar/100.0.0'].optional).toBeTruthy()
})

test('dependencies should be updated in the fields where they already are', async () => {
  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' })

  prepareEmpty()
  const manifest = await addDependenciesToPackage({
    devDependencies: {
      foo: '^100.0.0',
    },
    optionalDependencies: {
      bar: '^100.0.0',
    },
  }, ['foo@latest', 'bar@latest'], await testDefaults())

  expect(manifest).toStrictEqual({
    devDependencies: {
      foo: '^100.1.0',
    },
    optionalDependencies: {
      bar: '^100.1.0',
    },
  })
})

test('dependency should be removed from the old field when installing it as a different type of dependency', async () => {
  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'qar', version: '100.0.0', distTag: 'latest' })

  const project = prepareEmpty()
  let manifest = await addDependenciesToPackage({
    dependencies: {
      foo: '^100.0.0',
    },
    devDependencies: {
      bar: '^100.0.0',
    },
    optionalDependencies: {
      qar: '^100.0.0',
    },
  }, ['foo'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['bar'], await testDefaults({ targetDependenciesField: 'dependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['qar'], await testDefaults({ targetDependenciesField: 'devDependencies' }))

  expect(manifest).toStrictEqual({
    dependencies: {
      bar: '^100.0.0',
    },
    devDependencies: {
      qar: '^100.0.0',
    },
    optionalDependencies: {
      foo: '^100.0.0',
    },
  })

  manifest = await addDependenciesToPackage(manifest, ['bar', 'foo', 'qar'], await testDefaults({ targetDependenciesField: 'dependencies' }))

  expect(manifest).toStrictEqual({
    dependencies: {
      bar: '^100.0.0',
      foo: '^100.0.0',
      qar: '^100.0.0',
    },
    devDependencies: {},
    optionalDependencies: {},
  })

  {
    const lockfile = await project.readCurrentLockfile()
    expect(Object.keys(lockfile.dependencies)).toStrictEqual(['bar', 'foo', 'qar'])
  }

  console.log('manually editing package.json. Converting all prod deps to dev deps')

  manifest.devDependencies = manifest.dependencies
  delete manifest.dependencies

  await install(manifest, await testDefaults())

  {
    const lockfile = await project.readCurrentLockfile()
    expect(Object.keys(lockfile.devDependencies)).toStrictEqual(['bar', 'foo', 'qar'])
    expect(lockfile.dependencies).toBeFalsy()
  }
})

test('multiple save to package.json with `exact` versions (@rstacruz/tap-spec & rimraf@2.5.1) (in sorted order)', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0', '@zkochan/foo@latest'], await testDefaults({ save: true, pinnedVersion: 'patch' }))

  await project.has('@zkochan/foo')
  await project.has('is-positive')

  const expectedDeps = {
    '@zkochan/foo': '1.0.0',
    'is-positive': '1.0.0',
  }
  expect(manifest.dependencies).toStrictEqual(expectedDeps)
  expect(Object.keys(manifest.dependencies!).sort()).toStrictEqual(Object.keys(expectedDeps).sort())
})

test('save to package.json with save prefix ~', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ pinnedVersion: 'minor' }))

  expect(manifest.dependencies).toStrictEqual({ 'pkg-with-1-dep': '~100.0.0' })
})

test('an update bumps the versions in the manifest', async () => {
  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '2.0.0', distTag: 'latest' })

  prepareEmpty()

  const [{ manifest }] = await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          'peer-a': '~1.0.0',
        },
        devDependencies: {
          foo: '^100.0.0',
        },
        optionalDependencies: {
          'peer-c': '^1.0.1',
        },
      },
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ],
  await testDefaults({
    update: true,
  }))

  expect(manifest).toStrictEqual({
    dependencies: {
      'peer-a': '~1.0.1',
    },
    devDependencies: {
      foo: '^100.1.0',
    },
    optionalDependencies: {
      'peer-c': '^1.0.1',
    },
  })
})
