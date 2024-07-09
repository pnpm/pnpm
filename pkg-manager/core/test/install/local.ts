import fs from 'fs'
import path from 'path'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { type LockfileV9 as Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import { type ProjectRootDir } from '@pnpm/types'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
  type MutatedProject,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import { sync as rimraf } from '@zkochan/rimraf'
import normalizePath from 'normalize-path'
import { sync as readYamlFile } from 'read-yaml-file'
import symlinkDir from 'symlink-dir'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test('scoped modules from a directory', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [`file:${f.find('local-scoped-pkg')}`], testDefaults())

  const m = project.requireModule('@scope/local-scoped-pkg')

  expect(m()).toBe('@scope/local-scoped-pkg')
})

test('local file', async () => {
  const project = prepareEmpty()
  f.copy('local-pkg', path.resolve('..', 'local-pkg'))

  const manifest = await addDependenciesToPackage({}, ['link:../local-pkg'], testDefaults())

  const expectedSpecs = { 'local-pkg': `link:..${path.sep}local-pkg` }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)

  const m = project.requireModule('local-pkg')

  expect(m).toBeTruthy()

  const lockfile = project.readLockfile()

  expect(lockfile).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        dependencies: {
          'local-pkg': {
            specifier: expectedSpecs['local-pkg'],
            version: 'link:../local-pkg',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
  })
})

test('a symlink to a symlink to a local dependency is preserved', async () => {
  prepareEmpty()
  const localPkgDir = path.resolve('..', 'local-pkg')
  f.copy('local-pkg', localPkgDir)
  await symlinkDir(localPkgDir, path.resolve('../symlink'))

  await addDependenciesToPackage({}, ['link:../symlink'], testDefaults())

  expect(fs.readlinkSync(path.resolve('node_modules/local-pkg'))).toContain('symlink')
})

test('local directory with no package.json', async () => {
  const project = prepareEmpty()
  fs.mkdirSync('pkg')
  fs.writeFileSync('pkg/index.js', 'hello', 'utf8')

  const manifest = await addDependenciesToPackage({}, ['file:./pkg'], testDefaults())

  const expectedSpecs = { pkg: 'file:pkg' }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)
  project.has('pkg')

  rimraf('node_modules')

  await install(manifest, testDefaults({ frozenLockfile: true }))
  project.has('pkg')
})

test('local file via link:', async () => {
  const project = prepareEmpty()
  f.copy('local-pkg', path.resolve('..', 'local-pkg'))

  const manifest = await addDependenciesToPackage({}, ['link:../local-pkg'], testDefaults())

  const expectedSpecs = { 'local-pkg': `link:..${path.sep}local-pkg` }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)

  const m = project.requireModule('local-pkg')

  expect(m).toBeTruthy()

  const lockfile = project.readLockfile()

  expect(lockfile).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        dependencies: {
          'local-pkg': {
            specifier: expectedSpecs['local-pkg'],
            version: 'link:../local-pkg',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
  })
})

test('local file with symlinked node_modules', async () => {
  const project = prepareEmpty()
  f.copy('local-pkg', path.resolve('..', 'local-pkg'))
  fs.mkdirSync(path.join('..', 'node_modules'))
  await symlinkDir(path.join('..', 'node_modules'), 'node_modules')

  const manifest = await addDependenciesToPackage({}, ['link:../local-pkg'], testDefaults())

  const expectedSpecs = { 'local-pkg': `link:..${path.sep}local-pkg` }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)

  const m = project.requireModule('local-pkg')

  expect(m).toBeTruthy()

  const lockfile = project.readLockfile()

  expect(lockfile).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        dependencies: {
          'local-pkg': {
            specifier: expectedSpecs['local-pkg'],
            version: 'link:../local-pkg',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
  })
})

test('package with a broken symlink', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [f.find('has-broken-symlink/has-broken-symlink.tar.gz')], testDefaults({ fastUnpack: false }))

  const m = project.requireModule('has-broken-symlink')

  expect(m).toBeTruthy()
})

test('tarball local package', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, [f.find('tar-pkg/tar-pkg-1.0.0.tgz')], testDefaults({ fastUnpack: false }))

  const m = project.requireModule('tar-pkg')

  expect(m()).toBe('tar-pkg')

  const pkgSpec = `file:${normalizePath(f.find('tar-pkg/tar-pkg-1.0.0.tgz'))}`
  expect(manifest.dependencies).toStrictEqual({ 'tar-pkg': pkgSpec })

  const lockfile = project.readLockfile()
  expect(lockfile.packages[`tar-pkg@${lockfile.importers['.'].dependencies!['tar-pkg'].version}`]).toStrictEqual({
    resolution: {
      integrity: 'sha512-HP/5Rgt3pVFLzjmN9qJJ6vZMgCwoCIl/m2bPndYT283CUqnmFiMx0GeeIJ7SyK6TYoJM78SEvFEOQie++caHqw==',
      tarball: `file:${normalizePath(path.relative(process.cwd(), f.find('tar-pkg/tar-pkg-1.0.0.tgz')))}`,
    },
    version: '1.0.0',
  })
})

test('tarball local package from project directory', async () => {
  const project = prepareEmpty()

  f.copy('tar-pkg/tar-pkg-1.0.0.tgz', path.resolve('tar-pkg-1.0.0.tgz'))

  const manifest = await install({
    dependencies: {
      'tar-pkg': 'file:tar-pkg-1.0.0.tgz',
    },
  }, testDefaults({ fastUnpack: false }))

  const m = project.requireModule('tar-pkg')

  expect(m()).toBe('tar-pkg')

  const pkgSpec = 'file:tar-pkg-1.0.0.tgz'
  expect(manifest.dependencies).toStrictEqual({ 'tar-pkg': pkgSpec })

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.['tar-pkg'].version).toBe(pkgSpec)
  expect(lockfile.packages[`tar-pkg@${lockfile.importers['.'].dependencies!['tar-pkg'].version}`]).toStrictEqual({
    resolution: {
      integrity: 'sha512-HP/5Rgt3pVFLzjmN9qJJ6vZMgCwoCIl/m2bPndYT283CUqnmFiMx0GeeIJ7SyK6TYoJM78SEvFEOQie++caHqw==',
      tarball: pkgSpec,
    },
    version: '1.0.0',
  })
})

test('update tarball local package when its integrity changes', async () => {
  const project = prepareEmpty()

  f.copy('tar-pkg-with-dep-1/tar-pkg-with-dep-1.0.0.tgz', path.resolve('..', 'tar.tgz'))
  const manifest = await addDependenciesToPackage({}, ['../tar.tgz'], testDefaults())

  const lockfile1 = project.readLockfile()
  expect(lockfile1.snapshots['tar-pkg-with-dep@file:../tar.tgz'].dependencies!['is-positive']).toBe('1.0.0')

  f.copy('tar-pkg-with-dep-2/tar-pkg-with-dep-1.0.0.tgz', path.resolve('..', 'tar.tgz'))
  await install(manifest, testDefaults())

  const lockfile2 = project.readLockfile()
  expect(lockfile2.snapshots['tar-pkg-with-dep@file:../tar.tgz'].dependencies!['is-positive']).toBe('2.0.0')

  const manifestOfTarballDep = await import(path.resolve('node_modules/tar-pkg-with-dep/package.json'))
  expect(manifestOfTarballDep.dependencies['is-positive']).toBe('^2.0.0')
})

// Covers https://github.com/pnpm/pnpm/issues/1878
test('do not update deps when installing in a project that has local tarball dep', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  f.copy('tar-pkg-with-dep-1/tar-pkg-with-dep-1.0.0.tgz', path.resolve('..', 'tar.tgz'))
  const manifest = await addDependenciesToPackage({}, ['../tar.tgz', '@pnpm.e2e/peer-a'], testDefaults({ lockfileOnly: true }))

  const initialLockfile = project.readLockfile()

  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.1', distTag: 'latest' })

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults())

  const latestLockfile = project.readLockfile()

  expect(initialLockfile).toStrictEqual(latestLockfile)
})

// Covers https://github.com/pnpm/pnpm/issues/1882
test('frozen-lockfile: installation fails if the integrity of a tarball dependency changed', async () => {
  prepareEmpty()

  f.copy('tar-pkg-with-dep-1/tar-pkg-with-dep-1.0.0.tgz', path.resolve('..', 'tar.tgz'))
  const manifest = await addDependenciesToPackage({}, ['../tar.tgz'], testDefaults())

  rimraf('node_modules')

  f.copy('tar-pkg-with-dep-2/tar-pkg-with-dep-1.0.0.tgz', path.resolve('..', 'tar.tgz'))

  await expect(
    install(manifest, testDefaults({ frozenLockfile: true }))
  ).rejects.toThrow(/Got unexpected checksum/)
})

test('deep local', async () => {
  const manifest1 = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'project-2': 'file:../project-2',
    },
  }
  preparePackages([
    {
      location: 'project-1',
      package: manifest1,
    },
    {
      location: 'project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',
        dependencies: {
          'project-3': 'file:./project-3',
        },
      },
    },
    {
      location: 'project-2/project-3',
      package: {
        name: 'project-3',
        version: '1.0.0',
      },
    },
  ])
  process.chdir('project-1')
  await install(manifest1, testDefaults())

  const lockfile = readYamlFile<Lockfile>('pnpm-lock.yaml')
  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['project-2@file:../project-2', 'project-3@file:../project-2/project-3'])
})

// Covers https://github.com/pnpm/pnpm/issues/5327
test('resolution should not fail when a peer is resolved from a local package and there are many circular dependencies', async () => {
  const manifest1 = {
    name: 'chained-iterator',
    version: '0.0.4',
    dependencies: {
      '@bryntum/siesta': '6.0.0-beta-1',
    },
  }
  const manifest2 = {
    name: '@bryntum/chronograph',
    version: '2.0.3',
    dependencies: {
      '@bryntum/siesta': '6.0.0-beta-1',
      'typescript-serializable-mixin': '0.0.3',
      'typescript-mixin-class': 'link:../typescript-mixin-class',
    },
  }
  const manifest3 = {
    name: 'typescript-mixin-class',
    version: '0.0.3',
    dependencies: {
      '@bryntum/siesta': '6.0.0-beta-1',
    },
  }

  preparePackages([
    {
      location: manifest1.name,
      package: manifest1,
    },
    {
      location: manifest2.name,
      package: manifest2,
    },
    {
      location: manifest3.name,
      package: manifest3,
    },
  ])
  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve(manifest1.name) as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve(manifest2.name) as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve(manifest3.name) as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: manifest1,
      rootDir: path.resolve(manifest1.name) as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: manifest2,
      rootDir: path.resolve(manifest2.name) as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: manifest3,
      rootDir: path.resolve(manifest3.name) as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({ allProjects, lockfileOnly: true, strictPeerDependencies: false }))
  // All we need to know in this test is that installation doesn't fail
})

test('re-install should update local file dependency', async () => {
  const project = prepareEmpty()
  f.copy('local-pkg', path.resolve('..', 'local-pkg'))

  const manifest = await addDependenciesToPackage({}, ['file:../local-pkg'], testDefaults())

  const expectedSpecs = { 'local-pkg': `file:..${path.sep}local-pkg` }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)

  const m = project.requireModule('local-pkg')

  expect(m).toBeTruthy()
  expect(fs.existsSync('./node_modules/local-pkg/add.js')).toBeFalsy()

  let lockfile = project.readLockfile()

  expect(lockfile).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        dependencies: {
          'local-pkg': {
            specifier: expectedSpecs['local-pkg'],
            version: 'file:../local-pkg',
          },
        },
      },
    },
    packages: {
      'local-pkg@file:../local-pkg': {
        resolution: { directory: '../local-pkg', type: 'directory' },
      },
    },
    snapshots: {
      'local-pkg@file:../local-pkg': {},
    },
    lockfileVersion: LOCKFILE_VERSION,
  })

  // add file
  fs.writeFileSync('../local-pkg/add.js', 'added', 'utf8')
  await install(manifest, testDefaults())
  expect(fs.existsSync('./node_modules/local-pkg/add.js')).toBeTruthy()

  // remove file
  fs.rmSync('../local-pkg/add.js')
  await install(manifest, testDefaults())
  expect(fs.existsSync('./node_modules/local-pkg/add.js')).toBeFalsy()

  // add dependency
  expect(fs.existsSync('./node_modules/.pnpm/is-positive@1.0.0')).toBeFalsy()
  fs.writeFileSync('../local-pkg/package.json', JSON.stringify({
    name: 'local-pkg',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  }), 'utf8')
  await install(manifest, testDefaults())
  expect(fs.existsSync('./node_modules/.pnpm/is-positive@1.0.0')).toBeTruthy()
  lockfile = project.readLockfile()
  expect(lockfile).toMatchObject({
    snapshots: {
      'local-pkg@file:../local-pkg': {
        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
    packages: {
      'local-pkg@file:../local-pkg': {
        resolution: { directory: '../local-pkg', type: 'directory' },
      },
    },
  })

  // update dependency
  fs.writeFileSync('../local-pkg/package.json', JSON.stringify({
    name: 'local-pkg',
    version: '1.0.0',
    dependencies: {
      'is-positive': '2.0.0',
    },
  }), 'utf8')
  await install(manifest, testDefaults())
  expect(fs.existsSync('./node_modules/.pnpm/is-positive@2.0.0')).toBeTruthy()
  lockfile = project.readLockfile()
  expect(lockfile).toMatchObject({
    packages: {
      'local-pkg@file:../local-pkg': {
        resolution: { directory: '../local-pkg', type: 'directory' },
      },
    },
    snapshots: {
      'local-pkg@file:../local-pkg': {
        dependencies: {
          'is-positive': '2.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
  })
})

test('local directory is not relinked if disableRelinkLocalDirDeps is set to true', async () => {
  prepareEmpty()
  fs.mkdirSync('pkg')
  fs.writeFileSync('pkg/index.js', 'hello', 'utf8')
  fs.writeFileSync('pkg/package.json', '{"name": "pkg"}', 'utf8')

  const manifest = await addDependenciesToPackage({}, ['file:./pkg'], testDefaults())

  fs.writeFileSync('pkg/new.js', 'hello', 'utf8')

  await addDependenciesToPackage(manifest, ['is-odd@1.0.0'], testDefaults({ disableRelinkLocalDirDeps: true }))

  expect(fs.readdirSync('node_modules/pkg').sort()).toStrictEqual(['index.js', 'package.json'])

  await install(manifest, testDefaults({ frozenLockfile: true, disableRelinkLocalDirDeps: true }))

  expect(fs.readdirSync('node_modules/pkg').sort()).toStrictEqual(['index.js', 'package.json'])
})
