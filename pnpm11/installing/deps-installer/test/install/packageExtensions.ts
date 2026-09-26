import fs from 'node:fs'
import path from 'node:path'
import zlib from 'node:zlib'

import { expect, test } from '@jest/globals'
import { hashObject as _hashObject } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import { addDependenciesToPackage, install, mutateModules, mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import type { PackageExtension, ProjectId, ProjectManifest, ProjectRootDir, ReadPackageHook } from '@pnpm/types'

import {
  testDefaults,
} from '../utils/index.js'

function hashObject (obj: Record<string, unknown>): string {
  return `sha256-${_hashObject(obj)}`
}

test('manifests are extended with fields specified by packageExtensions', async () => {
  const project = prepareEmpty()

  const packageExtensions: Record<string, PackageExtension> = {
    'is-positive': {
      dependencies: {
        '@pnpm.e2e/bar': '100.1.0',
      },
    },
  }
  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['is-positive@1.0.0'],
    testDefaults({ packageExtensions })
  )

  {
    const lockfile = project.readLockfile()
    expect(lockfile.snapshots['is-positive@1.0.0'].dependencies?.['@pnpm.e2e/bar']).toBe('100.1.0')
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(hashObject({
      'is-positive': {
        dependencies: {
          '@pnpm.e2e/bar': '100.1.0',
        },
      },
    }))
    const currentLockfile = project.readCurrentLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(currentLockfile.packageExtensionsChecksum)
  }

  // The lockfile is updated if the overrides are changed
  packageExtensions['is-positive'].dependencies!['@pnpm.e2e/foobar'] = '100.0.0'
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ packageExtensions }))

  {
    const lockfile = project.readLockfile()
    expect(lockfile.snapshots['is-positive@1.0.0'].dependencies?.['@pnpm.e2e/foobar']).toBe('100.0.0')
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(hashObject({
      'is-positive': {
        dependencies: {
          '@pnpm.e2e/bar': '100.1.0',
          '@pnpm.e2e/foobar': '100.0.0',
        },
      },
    }))
    const currentLockfile = project.readCurrentLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(currentLockfile.packageExtensionsChecksum)
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true, packageExtensions }))

  {
    const lockfile = project.readLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(hashObject({
      'is-positive': {
        dependencies: {
          '@pnpm.e2e/bar': '100.1.0',
          '@pnpm.e2e/foobar': '100.0.0',
        },
      },
    }))
    const currentLockfile = project.readCurrentLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(currentLockfile.packageExtensionsChecksum)
  }

  packageExtensions['is-positive'].dependencies!['@pnpm.e2e/bar'] = '100.0.1'
  await expect(
    mutateModulesInSingleProject({
      manifest,
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    }, testDefaults({ frozenLockfile: true, packageExtensions }))
  ).rejects.toThrow(
    new PnpmError('LOCKFILE_CONFIG_MISMATCH',
      'Cannot proceed with the frozen installation. The current "packageExtensionsChecksum" configuration doesn\'t match the value found in the lockfile'
    )
  )
})

test('update does not save a dependency added by a readPackage hook', async () => {
  const project = prepareEmpty()
  const manifest = {
    name: 'project',
    version: '1.0.0',
  }
  const readPackage: ReadPackageHook = (hookedManifest) => ({
    ...hookedManifest,
    dependencies: {
      ...hookedManifest.dependencies,
      '@pnpm.e2e/foo': '1.0.0',
    },
  })
  const options = testDefaults({
    hooks: {
      readPackage: [readPackage],
    },
  })

  await install(manifest, options)

  const { updatedProject } = await mutateModulesInSingleProject({
    allowNew: false,
    dependencySelectors: ['@pnpm.e2e/foo@2.0.0'],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
    update: true,
    updatePackageManifest: true,
  }, {
    ...options,
    ignoreCurrentSpecifiers: true,
  })

  expect(updatedProject.manifest).toStrictEqual({
    name: 'project',
    version: '1.0.0',
  })
  expect(project.readLockfile().packages['@pnpm.e2e/foo@1.0.0']).toBeDefined()
  expect(project.readLockfile().packages['@pnpm.e2e/foo@2.0.0']).toBeUndefined()
})

test('update preserves the original dependency field when a readPackage hook moves it', async () => {
  const project = prepareEmpty()
  const manifest = {
    name: 'project',
    version: '1.0.0',
    devDependencies: {
      '@pnpm.e2e/foo': '^1.0.0',
    },
  }
  const readPackage: ReadPackageHook = (hookedManifest) => {
    const specifier = hookedManifest.devDependencies?.['@pnpm.e2e/foo'] ?? '^1.0.0'
    delete hookedManifest.devDependencies
    hookedManifest.dependencies = {
      ...hookedManifest.dependencies,
      '@pnpm.e2e/foo': specifier,
    }
    return hookedManifest
  }
  const options = testDefaults({
    hooks: {
      readPackage: [readPackage],
    },
  })

  await install(manifest, options)

  const { updatedProject } = await mutateModulesInSingleProject({
    allowNew: false,
    dependencySelectors: ['@pnpm.e2e/foo'],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
    update: true,
    updatePackageManifest: true,
    updateToLatest: true,
  }, options)

  expect(updatedProject.manifest).toStrictEqual(manifest)
  expect(project.readLockfile().importers['.'].dependencies?.['@pnpm.e2e/foo']).toBeDefined()
  expect(project.readLockfile().importers['.'].devDependencies?.['@pnpm.e2e/foo']).toBeUndefined()
})

test('update --latest stays within a dependency range rewritten by a readPackage hook', async () => {
  const project = prepareEmpty()
  const manifest = {
    name: 'project',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/foo': '^1.0.0',
    },
  }
  const readPackage: ReadPackageHook = (hookedManifest) => ({
    ...hookedManifest,
    dependencies: {
      ...hookedManifest.dependencies,
      '@pnpm.e2e/foo': '1.0.0',
    },
  })
  const options = testDefaults({
    hooks: {
      readPackage: [readPackage],
    },
  })

  await install(manifest, options)

  const { updatedProject } = await mutateModulesInSingleProject({
    allowNew: false,
    dependencySelectors: ['@pnpm.e2e/foo'],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
    update: true,
    updatePackageManifest: true,
    updateToLatest: true,
  }, options)

  expect(updatedProject.manifest).toStrictEqual(manifest)
  expect(project.readLockfile().importers['.'].dependencies?.['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '1.0.0',
    version: '1.0.0',
  })
})

test('update selectors use the hook specifier when current specifiers are ignored', async () => {
  const project = prepareEmpty()
  const manifest = {
    name: 'project',
    version: '1.0.0',
  }
  const readPackage: ReadPackageHook = (hookedManifest) => ({
    ...hookedManifest,
    dependencies: {
      ...hookedManifest.dependencies,
      '@pnpm.e2e/foo': '^1.0.0',
    },
  })
  const options = testDefaults({
    hooks: {
      readPackage: [readPackage],
    },
  })

  await install(manifest, options)

  const { updatedProject } = await mutateModulesInSingleProject({
    allowNew: false,
    dependencySelectors: ['@pnpm.e2e/foo@1.0.1'],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
    update: true,
    updatePackageManifest: true,
  }, {
    ...options,
    ignoreCurrentSpecifiers: true,
  })

  expect(updatedProject.manifest).toStrictEqual(manifest)
  expect(project.readLockfile().importers['.'].dependencies?.['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '^1.0.0',
    version: '1.3.0',
  })
})

test('add saves a dependency when the requested specifier matches a readPackage rewrite', async () => {
  prepareEmpty()
  const manifest = {
    name: 'project',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/foo': '^1.0.0',
    },
  }
  const readPackage: ReadPackageHook = (hookedManifest) => ({
    ...hookedManifest,
    dependencies: {
      ...hookedManifest.dependencies,
      '@pnpm.e2e/foo': '1.0.0',
    },
  })

  const { updatedProject } = await mutateModulesInSingleProject({
    allowNew: true,
    dependencySelectors: ['@pnpm.e2e/foo@1.0.0'],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
    updatePackageManifest: true,
  }, testDefaults({
    hooks: {
      readPackage: [readPackage],
    },
  }))

  expect(updatedProject.manifest.dependencies?.['@pnpm.e2e/foo']).toBe('1.0.0')
})

test('update keeps a workspace range a readPackage hook supplies', async () => {
  const project = prepareEmpty()
  const rootManifest = {
    name: 'root',
    version: '1.0.0',
  }
  // The workspace holds the local package at two versions, so the range decides which one the
  // importer gets.
  const localV1 = { name: 'local-dep', version: '1.0.0' }
  const localV2 = { name: 'local-dep', version: '2.0.0' }
  const readPackage: ReadPackageHook = (hookedManifest) => hookedManifest.name !== 'root'
    ? hookedManifest
    : {
      ...hookedManifest,
      dependencies: {
        ...hookedManifest.dependencies,
        'local-dep': 'workspace:^1.0.0',
      },
    }
  const workspacePackages = new Map([
    ['local-dep', new Map([
      ['1.0.0', { rootDir: path.resolve('local-v1') as ProjectRootDir, manifest: localV1 }],
      ['2.0.0', { rootDir: path.resolve('local-v2') as ProjectRootDir, manifest: localV2 }],
    ])],
  ])
  const allProjects = [
    { buildIndex: 0, manifest: rootManifest, rootDir: process.cwd() as ProjectRootDir },
    { buildIndex: 0, manifest: localV1, rootDir: path.resolve('local-v1') as ProjectRootDir },
    { buildIndex: 0, manifest: localV2, rootDir: path.resolve('local-v2') as ProjectRootDir },
  ]
  const options = testDefaults({
    allProjects,
    workspacePackages,
    hooks: {
      readPackage: [readPackage],
    },
  })

  await mutateModules([
    { mutation: 'install', rootDir: process.cwd() as ProjectRootDir },
    { mutation: 'install', rootDir: path.resolve('local-v1') as ProjectRootDir },
    { mutation: 'install', rootDir: path.resolve('local-v2') as ProjectRootDir },
  ], options)

  const { updatedProjects } = await mutateModules([
    { mutation: 'install', rootDir: process.cwd() as ProjectRootDir, update: true, updatePackageManifest: true },
    { mutation: 'install', rootDir: path.resolve('local-v1') as ProjectRootDir, update: true },
    { mutation: 'install', rootDir: path.resolve('local-v2') as ProjectRootDir, update: true },
  ], options)

  expect(updatedProjects[0].manifest).toStrictEqual(rootManifest)
  // An update mode would have the workspace picker take the newest local version, which the kept
  // `workspace:^1.0.0` would then contradict.
  expect(project.readLockfile().importers['.' as ProjectId].dependencies?.['local-dep']).toStrictEqual({
    specifier: 'workspace:^1.0.0',
    version: 'link:local-v1',
  })
})

test('update does not give a dependency added by a readPackage hook a catalog name in manual mode', async () => {
  const project = prepareEmpty()
  const manifest = {
    name: 'project',
    version: '1.0.0',
  }
  const readPackage: ReadPackageHook = (hookedManifest) => ({
    ...hookedManifest,
    dependencies: {
      ...hookedManifest.dependencies,
      '@pnpm.e2e/foo': '1.0.0',
    },
  })
  const options = testDefaults({
    // `manual` skips the catalog loop, so the name has to be dropped where every mode passes.
    catalogMode: 'manual',
    saveCatalogName: 'default',
    hooks: {
      readPackage: [readPackage],
    },
  })

  await install(manifest, options)

  const { updatedCatalogs, updatedProject } = await mutateModulesInSingleProject({
    allowNew: false,
    dependencySelectors: ['@pnpm.e2e/foo'],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
    update: true,
    updatePackageManifest: true,
  }, options)

  expect(updatedProject.manifest).toStrictEqual(manifest)
  expect(updatedCatalogs).toBeUndefined()
  expect(project.readLockfile().catalogs).toBeUndefined()

  await install(manifest, { ...options, frozenLockfile: true })
})

test('update does not put a dependency added by a readPackage hook in a catalog when none covers it', async () => {
  const project = prepareEmpty()
  const manifest = {
    name: 'project',
    version: '1.0.0',
  }
  const readPackage: ReadPackageHook = (hookedManifest) => ({
    ...hookedManifest,
    dependencies: {
      ...hookedManifest.dependencies,
      '@pnpm.e2e/foo': '1.0.0',
    },
  })
  const options = testDefaults({
    catalogMode: 'prefer',
    hooks: {
      readPackage: [readPackage],
    },
  })

  await install(manifest, options)

  const { updatedCatalogs, updatedProject } = await mutateModulesInSingleProject({
    allowNew: false,
    dependencySelectors: ['@pnpm.e2e/foo'],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
    update: true,
    updatePackageManifest: true,
  }, options)

  expect(updatedProject.manifest).toStrictEqual(manifest)
  // Naming a catalog for the dependency would put a snapshot in the lockfile, and the matching
  // catalog entry is not this run's to write, so the next frozen install would reject the pair.
  expect(updatedCatalogs).toBeUndefined()
  expect(project.readLockfile().catalogs).toBeUndefined()

  await install(manifest, { ...options, frozenLockfile: true })
})

test('update does not move a dependency added by a readPackage hook into a catalog', async () => {
  const project = prepareEmpty()
  const manifest = {
    name: 'project',
    version: '1.0.0',
  }
  const readPackage: ReadPackageHook = (hookedManifest) => ({
    ...hookedManifest,
    dependencies: {
      ...hookedManifest.dependencies,
      '@pnpm.e2e/foo': '1.0.0',
    },
  })
  const options = testDefaults({
    catalogs: {
      default: {
        '@pnpm.e2e/foo': '^1.0.0',
      },
    },
    catalogMode: 'prefer',
    hooks: {
      readPackage: [readPackage],
    },
  })

  await install(manifest, options)

  const { updatedProject } = await mutateModulesInSingleProject({
    allowNew: false,
    dependencySelectors: ['@pnpm.e2e/foo'],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
    update: true,
    updatePackageManifest: true,
  }, options)

  expect(updatedProject.manifest).toStrictEqual(manifest)
  // A catalog entry resolves on its own range, and nothing here may record that the dependency
  // follows it, so the dependency has to keep resolving on the specifier the hook supplies.
  expect(project.readLockfile().catalogs).toBeUndefined()
  expect(project.readLockfile().importers['.'].dependencies?.['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '1.0.0',
    version: '1.0.0',
  })

  await install(manifest, { ...options, frozenLockfile: true })
})

test('update does not save a dependency added by packageExtensions', async () => {
  const project = prepareEmpty()
  const manifest = {
    name: 'project',
    version: '1.0.0',
  }
  const packageExtensions: Record<string, PackageExtension> = {
    'project@*': {
      dependencies: {
        '@pnpm.e2e/foo': '1.0.0',
      },
    },
  }

  await install(manifest, testDefaults({ packageExtensions }))

  const { updatedProject } = await mutateModulesInSingleProject({
    allowNew: false,
    dependencySelectors: ['@pnpm.e2e/foo@2.0.0'],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
    update: true,
    updatePackageManifest: true,
  }, testDefaults({ packageExtensions }))

  expect(updatedProject.manifest).toStrictEqual({
    name: 'project',
    version: '1.0.0',
  })
  expect(project.readLockfile().packages['@pnpm.e2e/foo@1.0.0']).toBeDefined()
  expect(project.readLockfile().packages['@pnpm.e2e/foo@2.0.0']).toBeUndefined()
})

test('packageExtensionsChecksum does not change regardless of keys order', async () => {
  const project = prepareEmpty()

  const packageExtensions1: Record<string, PackageExtension> = {
    'is-odd': {
      peerDependencies: {
        'is-number': '*',
      },
    },
    'is-even': {
      peerDependencies: {
        'is-number': '*',
      },
    },
  }

  const packageExtensions2: Record<string, PackageExtension> = {
    'is-even': {
      peerDependencies: {
        'is-number': '*',
      },
    },
    'is-odd': {
      peerDependencies: {
        'is-number': '*',
      },
    },
  }

  const manifest = (): ProjectManifest => ({
    dependencies: {
      'is-even': '*',
      'is-odd': '*',
    },
  })

  await install(manifest(), testDefaults({
    packageExtensions: packageExtensions1,
  }))
  const lockfile1 = project.readLockfile()
  const checksum1 = lockfile1.packageExtensionsChecksum

  await install(manifest(), testDefaults({
    packageExtensions: packageExtensions2,
  }))
  const lockfile2 = project.readLockfile()
  const checksum2 = lockfile2.packageExtensionsChecksum

  expect(checksum1).toBe(checksum2)
  expect(checksum1).not.toBeFalsy()
})

test('manifests are patched by extensions from the compatibility database', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['debug@4.0.0'],
    testDefaults()
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages['debug@4.0.0'].peerDependenciesMeta?.['supports-color']?.optional).toBe(true)
})

test('manifests are not patched by extensions from the compatibility database when ignoreCompatibilityDb is true', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['debug@4.0.0'],
    testDefaults({
      ignoreCompatibilityDb: true,
    })
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages['debug@4.0.0'].peerDependenciesMeta).toBeUndefined()
})

test('manifests without version do not match ranged packageExtensions selectors', async () => {
  const project = prepareEmpty()

  const tarballPath = path.resolve('no-manifest-1.0.0.tgz')
  fs.writeFileSync(tarballPath, createTarGz([{ name: 'package/README.md', content: 'placeholder' }]))

  const packageExtensions: Record<string, PackageExtension> = {
    'no-manifest@<2': {
      dependencies: {
        '@pnpm.e2e/bar': '100.1.0',
      },
    },
    'no-manifest@*': {
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    'no-manifest@>=100': {
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    'no-manifest': {
      dependencies: {
        '@pnpm.e2e/foobar': '100.0.0',
      },
    },
  }

  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    [`no-manifest@file:${tarballPath}`],
    testDefaults({
      packageExtensions,
    })
  )

  {
    const lockfile = project.readLockfile()
    const depKey = Object.keys(lockfile.snapshots).find((key) => key.startsWith('no-manifest@'))!
    const snapshot = lockfile.snapshots[depKey]
    // Ranged selectors (@<2, @*, @>=100) must not match synthesized/absent version
    expect(snapshot.dependencies?.['@pnpm.e2e/bar']).toBeUndefined()
    expect(snapshot.dependencies?.['is-positive']).toBeUndefined()
    expect(snapshot.dependencies?.['is-negative']).toBeUndefined()
    // Bare selector must match
    expect(snapshot.dependencies?.['@pnpm.e2e/foobar']).toBe('100.0.0')
  }

  // Second install with existing lockfile (where currentPkg.version is synthesized 0.0.0)
  await addDependenciesToPackage(
    manifest,
    [],
    testDefaults({
      packageExtensions,
    })
  )

  {
    const lockfile = project.readLockfile()
    const depKey = Object.keys(lockfile.snapshots).find((key) => key.startsWith('no-manifest@'))!
    const snapshot = lockfile.snapshots[depKey]
    expect(snapshot.dependencies?.['@pnpm.e2e/bar']).toBeUndefined()
    expect(snapshot.dependencies?.['is-positive']).toBeUndefined()
    expect(snapshot.dependencies?.['is-negative']).toBeUndefined()
    expect(snapshot.dependencies?.['@pnpm.e2e/foobar']).toBe('100.0.0')
  }
})

test('ranged packageExtensions selectors do not match a local directory with no package.json', async () => {
  const project = prepareEmpty()
  fs.mkdirSync('debug')
  fs.writeFileSync('debug/index.js', '', 'utf8')

  await addDependenciesToPackage({}, ['file:./debug'], testDefaults({
    packageExtensions: {
      'debug@<1': {
        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      debug: {
        dependencies: {
          'is-negative': '1.0.0',
        },
      },
    },
  }))

  expect(project.readLockfile().snapshots['debug@file:debug']).toStrictEqual({
    dependencies: {
      'is-negative': '1.0.0',
    },
  })
})

test('built-in compatibility database extensions are not applied to workspace project manifests', async () => {
  const project = prepareEmpty()

  const manifest = {
    name: 'vue-loader',
    version: '0.0.0',
  }

  await install(manifest, testDefaults())

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies).toBeUndefined()
  expect(lockfile.importers['.'].devDependencies).toBeUndefined()
  expect(lockfile.importers['.'].optionalDependencies).toBeUndefined()
})

function createTarGz (entries: Array<{ name: string, content: string | Buffer }>): Buffer {
  const blocks: Buffer[] = []
  for (const entry of entries) {
    const header = Buffer.alloc(512)
    header.write(entry.name, 0, 100, 'utf8')
    header.write('0000644\0', 100, 8, 'ascii')
    header.write('0000000\0', 108, 8, 'ascii')
    header.write('0000000\0', 116, 8, 'ascii')
    const content = Buffer.isBuffer(entry.content) ? entry.content : Buffer.from(entry.content, 'utf8')
    header.write(content.length.toString(8).padStart(11, '0') + '\0', 124, 12, 'ascii')
    header.write(Math.floor(Date.now() / 1000).toString(8).padStart(11, '0') + '\0', 136, 12, 'ascii')
    header.fill(32, 148, 156)
    header.write('0', 156, 1, 'ascii')
    header.write('ustar\0', 257, 6, 'ascii')
    header.write('00', 263, 2, 'ascii')

    let checksum = 0
    for (let i = 0; i < 512; i++) checksum += header[i]
    header.write(checksum.toString(8).padStart(6, '0') + '\0 ', 148, 8, 'ascii')

    blocks.push(header)
    blocks.push(content)
    const remainder = content.length % 512
    if (remainder !== 0) {
      blocks.push(Buffer.alloc(512 - remainder))
    }
  }
  blocks.push(Buffer.alloc(1024))
  return zlib.gzipSync(Buffer.concat(blocks))
}

