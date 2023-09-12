import fss, { promises as fs } from 'fs'
import path from 'path'
import { LOCKFILE_VERSION_V6 as LOCKFILE_VERSION } from '@pnpm/constants'
import { type Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
  type MutatedProject,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import rimraf from '@zkochan/rimraf'
import normalizePath from 'normalize-path'
import readYamlFile from 'read-yaml-file'
import symlinkDir from 'symlink-dir'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test('scoped modules from a directory', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [`file:${f.find('local-scoped-pkg')}`], await testDefaults())

  const m = project.requireModule('@scope/local-scoped-pkg')

  expect(m()).toBe('@scope/local-scoped-pkg')
})

test('local file', async () => {
  const project = prepareEmpty()
  f.copy('local-pkg', path.resolve('..', 'local-pkg'))

  const manifest = await addDependenciesToPackage({}, ['link:../local-pkg'], await testDefaults())

  const expectedSpecs = { 'local-pkg': `link:..${path.sep}local-pkg` }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)

  const m = project.requireModule('local-pkg')

  expect(m).toBeTruthy()

  const lockfile = await project.readLockfile()

  expect(lockfile).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    dependencies: {
      'local-pkg': {
        specifier: expectedSpecs['local-pkg'],
        version: 'link:../local-pkg',
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

  await addDependenciesToPackage({}, ['link:../symlink'], await testDefaults())

  expect(await fs.readlink(path.resolve('node_modules/local-pkg'))).toContain('symlink')
})

test('local directory with no package.json', async () => {
  const project = prepareEmpty()
  await fs.mkdir('pkg')
  await fs.writeFile('pkg/index.js', 'hello', 'utf8')

  const manifest = await addDependenciesToPackage({}, ['file:./pkg'], await testDefaults())

  const expectedSpecs = { pkg: 'file:pkg' }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)
  await project.has('pkg')

  await rimraf('node_modules')

  await install(manifest, await testDefaults({ frozenLockfile: true }))
  await project.has('pkg')
})

test('local file via link:', async () => {
  const project = prepareEmpty()
  f.copy('local-pkg', path.resolve('..', 'local-pkg'))

  const manifest = await addDependenciesToPackage({}, ['link:../local-pkg'], await testDefaults())

  const expectedSpecs = { 'local-pkg': `link:..${path.sep}local-pkg` }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)

  const m = project.requireModule('local-pkg')

  expect(m).toBeTruthy()

  const lockfile = await project.readLockfile()

  expect(lockfile).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    dependencies: {
      'local-pkg': {
        specifier: expectedSpecs['local-pkg'],
        version: 'link:../local-pkg',
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
  })
})

test('local file with symlinked node_modules', async () => {
  const project = prepareEmpty()
  f.copy('local-pkg', path.resolve('..', 'local-pkg'))
  await fs.mkdir(path.join('..', 'node_modules'))
  await symlinkDir(path.join('..', 'node_modules'), 'node_modules')

  const manifest = await addDependenciesToPackage({}, ['link:../local-pkg'], await testDefaults())

  const expectedSpecs = { 'local-pkg': `link:..${path.sep}local-pkg` }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)

  const m = project.requireModule('local-pkg')

  expect(m).toBeTruthy()

  const lockfile = await project.readLockfile()

  expect(lockfile).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    dependencies: {
      'local-pkg': {
        specifier: expectedSpecs['local-pkg'],
        version: 'link:../local-pkg',
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
  })
})

test('package with a broken symlink', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [f.find('has-broken-symlink/has-broken-symlink.tar.gz')], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('has-broken-symlink')

  expect(m).toBeTruthy()
})

test('tarball local package', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, [f.find('tar-pkg/tar-pkg-1.0.0.tgz')], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('tar-pkg')

  expect(m()).toBe('tar-pkg')

  const pkgSpec = `file:${normalizePath(f.find('tar-pkg/tar-pkg-1.0.0.tgz'))}`
  expect(manifest.dependencies).toStrictEqual({ 'tar-pkg': pkgSpec })

  const lockfile = await project.readLockfile()
  expect(lockfile.packages[lockfile.dependencies['tar-pkg'].version]).toStrictEqual({
    dev: false,
    name: 'tar-pkg',
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
  }, await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('tar-pkg')

  expect(m()).toBe('tar-pkg')

  const pkgSpec = 'file:tar-pkg-1.0.0.tgz'
  expect(manifest.dependencies).toStrictEqual({ 'tar-pkg': pkgSpec })

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['tar-pkg'].version).toBe(pkgSpec)
  expect(lockfile.packages[lockfile.dependencies['tar-pkg'].version]).toStrictEqual({
    dev: false,
    name: 'tar-pkg',
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
  const manifest = await addDependenciesToPackage({}, ['../tar.tgz'], await testDefaults())

  const lockfile1 = await project.readLockfile()
  expect(lockfile1.packages['file:../tar.tgz'].dependencies!['is-positive']).toBe('1.0.0')

  f.copy('tar-pkg-with-dep-2/tar-pkg-with-dep-1.0.0.tgz', path.resolve('..', 'tar.tgz'))
  await install(manifest, await testDefaults())

  const lockfile2 = await project.readLockfile()
  expect(lockfile2.packages['file:../tar.tgz'].dependencies!['is-positive']).toBe('2.0.0')

  const manifestOfTarballDep = await import(path.resolve('node_modules/tar-pkg-with-dep/package.json'))
  expect(manifestOfTarballDep.dependencies['is-positive']).toBe('^2.0.0')
})

// Covers https://github.com/pnpm/pnpm/issues/1878
test('do not update deps when installing in a project that has local tarball dep', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  f.copy('tar-pkg-with-dep-1/tar-pkg-with-dep-1.0.0.tgz', path.resolve('..', 'tar.tgz'))
  const manifest = await addDependenciesToPackage({}, ['../tar.tgz', '@pnpm.e2e/peer-a'], await testDefaults({ lockfileOnly: true }))

  const initialLockfile = await project.readLockfile()

  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.1', distTag: 'latest' })

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults())

  const latestLockfile = await project.readLockfile()

  expect(initialLockfile).toStrictEqual(latestLockfile)
})

// Covers https://github.com/pnpm/pnpm/issues/1882
test('frozen-lockfile: installation fails if the integrity of a tarball dependency changed', async () => {
  prepareEmpty()

  f.copy('tar-pkg-with-dep-1/tar-pkg-with-dep-1.0.0.tgz', path.resolve('..', 'tar.tgz'))
  const manifest = await addDependenciesToPackage({}, ['../tar.tgz'], await testDefaults())

  await rimraf('node_modules')

  f.copy('tar-pkg-with-dep-2/tar-pkg-with-dep-1.0.0.tgz', path.resolve('..', 'tar.tgz'))

  await expect(
    install(manifest, await testDefaults({ frozenLockfile: true }))
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
  await install(manifest1, await testDefaults())

  const lockfile = await readYamlFile<Lockfile>('pnpm-lock.yaml')
  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['file:../project-2', 'file:../project-2/project-3'])
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
      rootDir: path.resolve(manifest1.name),
    },
    {
      mutation: 'install',
      rootDir: path.resolve(manifest2.name),
    },
    {
      mutation: 'install',
      rootDir: path.resolve(manifest3.name),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: manifest1,
      rootDir: path.resolve(manifest1.name),
    },
    {
      buildIndex: 0,
      manifest: manifest2,
      rootDir: path.resolve(manifest2.name),
    },
    {
      buildIndex: 0,
      manifest: manifest3,
      rootDir: path.resolve(manifest3.name),
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects, lockfileOnly: true, strictPeerDependencies: false }))
  // All we need to know in this test is that installation doesn't fail
})

test('re-install should update local file dependency', async () => {
  const project = prepareEmpty()
  f.copy('local-pkg', path.resolve('..', 'local-pkg'))

  const manifest = await addDependenciesToPackage({}, ['file:../local-pkg'], await testDefaults())

  const expectedSpecs = { 'local-pkg': `file:..${path.sep}local-pkg` }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)

  const m = project.requireModule('local-pkg')

  expect(m).toBeTruthy()
  await expect(fs.access('./node_modules/local-pkg/add.js')).rejects.toThrow()

  let lockfile = await project.readLockfile()

  expect(lockfile).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    dependencies: {
      'local-pkg': {
        specifier: expectedSpecs['local-pkg'],
        version: 'file:../local-pkg',
      },
    },
    packages: {
      'file:../local-pkg': {
        resolution: { directory: '../local-pkg', type: 'directory' },
        name: 'local-pkg',
        dev: false,
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
  })

  // add file
  await fs.writeFile('../local-pkg/add.js', 'added', 'utf8')
  await install(manifest, await testDefaults())
  await expect(fs.access('./node_modules/local-pkg/add.js')).resolves.toBeUndefined()

  // remove file
  await fs.rm('../local-pkg/add.js')
  await install(manifest, await testDefaults())
  await expect(fs.access('./node_modules/local-pkg/add.js')).rejects.toThrow()

  // add dependency
  await expect(fs.access('./node_modules/.pnpm/is-positive@1.0.0')).rejects.toThrow()
  await fs.writeFile('../local-pkg/package.json', JSON.stringify({
    name: 'local-pkg',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  }), 'utf8')
  await install(manifest, await testDefaults())
  await expect(fs.access('./node_modules/.pnpm/is-positive@1.0.0')).resolves.toBeUndefined()
  lockfile = await project.readLockfile()
  expect(lockfile).toMatchObject({
    packages: {
      'file:../local-pkg': {
        resolution: { directory: '../local-pkg', type: 'directory' },
        name: 'local-pkg',
        dev: false,
        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
  })

  // update dependency
  await fs.writeFile('../local-pkg/package.json', JSON.stringify({
    name: 'local-pkg',
    version: '1.0.0',
    dependencies: {
      'is-positive': '2.0.0',
    },
  }), 'utf8')
  await install(manifest, await testDefaults())
  await expect(fs.access('./node_modules/.pnpm/is-positive@2.0.0')).resolves.toBeUndefined()
  lockfile = await project.readLockfile()
  expect(lockfile).toMatchObject({
    packages: {
      'file:../local-pkg': {
        resolution: { directory: '../local-pkg', type: 'directory' },
        name: 'local-pkg',
        dev: false,
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
  fss.mkdirSync('pkg')
  fss.writeFileSync('pkg/index.js', 'hello', 'utf8')
  fss.writeFileSync('pkg/package.json', '{"name": "pkg"}', 'utf8')

  const manifest = await addDependenciesToPackage({}, ['file:./pkg'], await testDefaults())

  fss.writeFileSync('pkg/new.js', 'hello', 'utf8')

  await addDependenciesToPackage(manifest, ['is-odd@1.0.0'], await testDefaults({ disableRelinkLocalDirDeps: true }))

  expect(fss.readdirSync('node_modules/pkg').sort()).toStrictEqual(['index.js', 'package.json'])

  await install(manifest, await testDefaults({ frozenLockfile: true, disableRelinkLocalDirDeps: true }))

  expect(fss.readdirSync('node_modules/pkg').sort()).toStrictEqual(['index.js', 'package.json'])
})
