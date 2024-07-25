import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import { addDistTag } from '@pnpm/registry-mock'
import { type ProjectRootDir } from '@pnpm/types'
import { testDefaults } from '../utils'

test('save to package.json (is-positive@^1.0.0)', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['is-positive@^1.0.0'], testDefaults({ save: true }))

  project.has('is-positive')

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
  }, ['is-positive'], testDefaults())
  manifest = await addDependenciesToPackage(manifest, ['is-negative'], testDefaults())
  manifest = await addDependenciesToPackage(manifest, ['sec'], testDefaults())

  expect(project.requireModule('is-positive/package.json').version).toBe('2.0.0')
  expect(project.requireModule('is-negative/package.json').version).toBe('1.0.1')

  expect(manifest.dependencies).toStrictEqual({
    'is-negative': '^1.0.1',
    'is-positive': '^2.0.0',
    sec: 'sindresorhus/sec#main',
  })
})

test('saveDev scoped module to package.json (@rstacruz/tap-spec)', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['@rstacruz/tap-spec'], testDefaults({ fastUnpack: false, targetDependenciesField: 'devDependencies' }))

  const m = project.requireModule('@rstacruz/tap-spec')
  expect(typeof m).toBe('function')

  expect(manifest.devDependencies).toStrictEqual({ '@rstacruz/tap-spec': '^4.1.1' })
})

test('dependency should not be added to package.json if it is already there', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })

  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({
    devDependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
    optionalDependencies: {
      '@pnpm.e2e/bar': '^100.0.0',
    },
  }, ['@pnpm.e2e/foo', '@pnpm.e2e/bar'], testDefaults())

  expect(manifest).toStrictEqual({
    devDependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
    optionalDependencies: {
      '@pnpm.e2e/bar': '^100.0.0',
    },
  })

  const lockfile = project.readLockfile()

  expect(lockfile.importers['.'].devDependencies?.['@pnpm.e2e/foo'].version).toBe('100.0.0')

  expect(lockfile.importers['.'].optionalDependencies?.['@pnpm.e2e/bar'].version).toBe('100.0.0')
  expect(lockfile.snapshots['@pnpm.e2e/bar@100.0.0'].optional).toBeTruthy()
})

test('dependencies should be updated in the fields where they already are', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  prepareEmpty()
  const manifest = await addDependenciesToPackage({
    devDependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
    optionalDependencies: {
      '@pnpm.e2e/bar': '^100.0.0',
    },
  }, ['@pnpm.e2e/foo@latest', '@pnpm.e2e/bar@latest'], testDefaults())

  expect(manifest).toStrictEqual({
    devDependencies: {
      '@pnpm.e2e/foo': '^100.1.0',
    },
    optionalDependencies: {
      '@pnpm.e2e/bar': '^100.1.0',
    },
  })
})

test('dependency should be removed from the old field when installing it as a different type of dependency', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/qar', version: '100.0.0', distTag: 'latest' })

  const project = prepareEmpty()
  let manifest = await addDependenciesToPackage({
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/bar': '^100.0.0',
    },
    optionalDependencies: {
      '@pnpm.e2e/qar': '^100.0.0',
    },
  }, ['@pnpm.e2e/foo'], testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['@pnpm.e2e/bar'], testDefaults({ targetDependenciesField: 'dependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['@pnpm.e2e/qar'], testDefaults({ targetDependenciesField: 'devDependencies' }))

  expect(manifest).toStrictEqual({
    dependencies: {
      '@pnpm.e2e/bar': '^100.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/qar': '^100.0.0',
    },
    optionalDependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })

  manifest = await addDependenciesToPackage(manifest, ['@pnpm.e2e/bar', '@pnpm.e2e/foo', '@pnpm.e2e/qar'], testDefaults({ targetDependenciesField: 'dependencies' }))

  expect(manifest).toStrictEqual({
    dependencies: {
      '@pnpm.e2e/bar': '^100.0.0',
      '@pnpm.e2e/foo': '^100.0.0',
      '@pnpm.e2e/qar': '^100.0.0',
    },
    devDependencies: {},
    optionalDependencies: {},
  })

  {
    const lockfile = project.readCurrentLockfile()
    expect(Object.keys(lockfile.importers['.'].dependencies ?? {})).toStrictEqual(['@pnpm.e2e/bar', '@pnpm.e2e/foo', '@pnpm.e2e/qar'])
  }

  // manually editing package.json. Converting all prod deps to dev deps

  manifest.devDependencies = manifest.dependencies
  delete manifest.dependencies

  await install(manifest, testDefaults())

  {
    const lockfile = project.readCurrentLockfile()
    expect(Object.keys(lockfile.importers['.'].devDependencies ?? {})).toStrictEqual(['@pnpm.e2e/bar', '@pnpm.e2e/foo', '@pnpm.e2e/qar'])
    expect(lockfile.dependencies).toBeFalsy()
  }
})

test('multiple save to package.json with `exact` versions (@rstacruz/tap-spec & rimraf@2.5.1) (in sorted order)', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0', '@zkochan/foo@latest'], testDefaults({ save: true, pinnedVersion: 'patch' }))

  project.has('@zkochan/foo')
  project.has('is-positive')

  const expectedDeps = {
    '@zkochan/foo': '1.0.0',
    'is-positive': '1.0.0',
  }
  expect(manifest.dependencies).toStrictEqual(expectedDeps)
  expect(Object.keys(manifest.dependencies!).sort()).toStrictEqual(Object.keys(expectedDeps).sort())
})

test('save to package.json with save prefix ~', async () => {
  await addDistTag({ package: '@pnpm.e2e/pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({ pinnedVersion: 'minor' }))

  expect(manifest.dependencies).toStrictEqual({ '@pnpm.e2e/pkg-with-1-dep': '~100.0.0' })
})

test('an update bumps the versions in the manifest', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '2.0.0', distTag: 'latest' })

  prepareEmpty()

  const { manifest } = await mutateModulesInSingleProject({
    manifest: {
      dependencies: {
        '@pnpm.e2e/peer-a': '~1.0.0',
      },
      devDependencies: {
        '@pnpm.e2e/foo': '^100.0.0',
      },
      optionalDependencies: {
        '@pnpm.e2e/peer-c': '^1.0.1',
      },
    },
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
    update: true,
  },
  testDefaults())

  expect(manifest).toStrictEqual({
    dependencies: {
      '@pnpm.e2e/peer-a': '~1.0.1',
    },
    devDependencies: {
      '@pnpm.e2e/foo': '^100.1.0',
    },
    optionalDependencies: {
      '@pnpm.e2e/peer-c': '^1.0.1',
    },
  })
})
