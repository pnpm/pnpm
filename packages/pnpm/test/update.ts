import path from 'path'
import { prepare, preparePackages } from '@pnpm/prepare'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import readYamlFile from 'read-yaml-file'
import writeYamlFile from 'write-yaml-file'
import {
  addDistTag,
  execPnpm,
} from './utils'

test('update <dep>', async () => {
  const project = prepare()

  await addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0', 'latest')

  await execPnpm(['install', '@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0'])

  await project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')

  await execPnpm(['update', '@pnpm.e2e/dep-of-pkg-with-1-dep@latest'])

  await project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0')

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('101.0.0')

  const pkg = await readPackageJsonFromDir(process.cwd())
  expect(pkg.dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('^101.0.0')
})

test('update --no-save', async () => {
  await addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest')
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })

  await execPnpm(['update', '--no-save'])

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/foo/100.1.0'])

  const pkg = await readPackageJsonFromDir(process.cwd())
  expect(pkg.dependencies?.['@pnpm.e2e/foo']).toBe('^100.0.0')
})

test('update', async () => {
  await addDistTag('@pnpm.e2e/foo', '100.0.0', 'latest')
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })

  await execPnpm(['install', '--lockfile-only'])

  await addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest')

  await execPnpm(['update'])

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/foo/100.1.0'])

  const pkg = await readPackageJsonFromDir(process.cwd())
  expect(pkg.dependencies?.['@pnpm.e2e/foo']).toBe('^100.1.0')
})

test('recursive update --no-save', async () => {
  await addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest')
  preparePackages([
    {
      location: 'project',
      package: {
        dependencies: {
          '@pnpm.e2e/foo': '^100.0.0',
        },
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'update', '--no-save'])

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // eslint-disable-line
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/foo/100.1.0'])

  const pkg = await readPackageJsonFromDir(path.resolve('project'))
  expect(pkg.dependencies?.['@pnpm.e2e/foo']).toBe('^100.0.0')
})

test('recursive update', async () => {
  await addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest')
  preparePackages([
    {
      location: 'project',
      package: {
        dependencies: {
          '@pnpm.e2e/foo': '^100.0.0',
        },
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'update'])

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // eslint-disable-line
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/foo/100.1.0'])

  const pkg = await readPackageJsonFromDir(path.resolve('project'))
  expect(pkg.dependencies?.['@pnpm.e2e/foo']).toBe('^100.1.0')
})

test('recursive update --no-shared-workspace-lockfile', async function () {
  await addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest')
  const projects = preparePackages([
    {
      location: 'project',
      package: {
        name: 'project',

        dependencies: {
          '@pnpm.e2e/foo': '^100.0.0',
        },
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'update', '--no-shared-workspace-lockfile'])

  const lockfile = await projects['project'].readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/foo/100.1.0'])

  const pkg = await readPackageJsonFromDir(path.resolve('project'))
  expect(pkg.dependencies?.['@pnpm.e2e/foo']).toBe('^100.1.0')
})

test('update --latest', async function () {
  const project = prepare()

  await Promise.all([
    addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('@pnpm.e2e/bar', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/qar', '100.1.0', 'latest'),
  ])

  await execPnpm(['add', '@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0', '@pnpm.e2e/bar@^100.0.0', 'alias@npm:@pnpm.e2e/qar@^100.0.0', 'kevva/is-negative'])

  await execPnpm(['update', '--latest'])

  await project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0')

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile.dependencies['@pnpm.e2e/bar']).toBe('100.1.0')
  expect(lockfile.dependencies['alias']).toBe('/@pnpm.e2e/qar/100.1.0')

  const pkg = await readPackageJsonFromDir(process.cwd())
  expect(pkg.dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('^101.0.0')
  expect(pkg.dependencies?.['@pnpm.e2e/bar']).toBe('^100.1.0')
  expect(pkg.dependencies?.['alias']).toBe('npm:@pnpm.e2e/qar@^100.1.0')
  expect(pkg.dependencies?.['is-negative']).toBe('github:kevva/is-negative')
})

test('update --latest --save-exact', async function () {
  const project = prepare()

  await Promise.all([
    addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('@pnpm.e2e/bar', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/qar', '100.1.0', 'latest'),
  ])

  await execPnpm(['install', '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0', '@pnpm.e2e/bar@100.0.0', 'alias@npm:@pnpm.e2e/qar@100.0.0', 'kevva/is-negative'])

  await execPnpm(['update', '--latest', '--save-exact'])

  await project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0')

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile.dependencies['@pnpm.e2e/bar']).toBe('100.1.0')
  expect(lockfile.dependencies['alias']).toBe('/@pnpm.e2e/qar/100.1.0')

  const pkg = await readPackageJsonFromDir(process.cwd())
  expect(pkg.dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(pkg.dependencies?.['@pnpm.e2e/bar']).toBe('100.1.0')
  expect(pkg.dependencies?.['alias']).toBe('npm:@pnpm.e2e/qar@100.1.0')
  expect(pkg.dependencies?.['is-negative']).toBe('github:kevva/is-negative')
})

test('update --latest specific dependency', async function () {
  const project = prepare()

  await Promise.all([
    addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('@pnpm.e2e/bar', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/qar', '100.1.0', 'latest'),
  ])

  await execPnpm(['add', '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0', '@pnpm.e2e/bar@^100.0.0', '@pnpm.e2e/foo@100.1.0', 'alias@npm:@pnpm.e2e/qar@^100.0.0', 'kevva/is-negative'])

  await execPnpm(['update', '-L', '@pnpm.e2e/bar', '@pnpm.e2e/foo@100.0.0', 'alias', 'is-negative'])

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(lockfile.dependencies['@pnpm.e2e/bar']).toBe('100.1.0')
  expect(lockfile.dependencies['@pnpm.e2e/foo']).toBe('100.0.0')
  expect(lockfile.dependencies['alias']).toBe('/@pnpm.e2e/qar/100.1.0')

  const pkg = await readPackageJsonFromDir(process.cwd())
  expect(pkg.dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(pkg.dependencies?.['@pnpm.e2e/bar']).toBe('^100.1.0')
  expect(pkg.dependencies?.['@pnpm.e2e/foo']).toBe('100.0.0')
  expect(pkg.dependencies?.['alias']).toBe('npm:@pnpm.e2e/qar@^100.1.0')
  expect(pkg.dependencies?.['is-negative']).toBe('github:kevva/is-negative')
})

test('update --latest --prod', async function () {
  const project = prepare()

  await Promise.all([
    addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('@pnpm.e2e/bar', '100.1.0', 'latest'),
  ])

  await execPnpm(['add', '-D', '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  await execPnpm(['add', '-P', '@pnpm.e2e/bar@^100.0.0'])

  await execPnpm(['update', '--latest', '--prod'])

  const lockfile = await project.readLockfile()
  expect(lockfile.devDependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(lockfile.dependencies['@pnpm.e2e/bar']).toBe('100.1.0')

  const pkg = await readPackageJsonFromDir(process.cwd())
  expect(pkg.devDependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(pkg.dependencies?.['@pnpm.e2e/bar']).toBe('^100.1.0')

  await project.has('@pnpm.e2e/dep-of-pkg-with-1-dep') // not pruned
})

test('recursive update --latest on projects that do not share a lockfile', async () => {
  await Promise.all([
    addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('@pnpm.e2e/bar', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest'),
  ])

  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/bar': '100.0.0',
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
  ])

  await execPnpm(['recursive', 'install'])

  await execPnpm(['recursive', 'update', '--latest'])

  const manifest1 = await readPackageJsonFromDir(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    '@pnpm.e2e/foo': '100.1.0',
  })

  const lockfile1 = await projects['project-1'].readLockfile()
  expect(lockfile1.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile1.dependencies['@pnpm.e2e/foo']).toBe('100.1.0')

  const manifest2 = await readPackageJsonFromDir(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    '@pnpm.e2e/bar': '100.1.0',
    '@pnpm.e2e/foo': '100.1.0',
  })

  const lockfile2 = await projects['project-2'].readLockfile()
  expect(lockfile2.dependencies['@pnpm.e2e/bar']).toBe('100.1.0')
  expect(lockfile2.dependencies['@pnpm.e2e/foo']).toBe('100.1.0')
})

test('recursive update --latest --prod on projects that do not share a lockfile', async () => {
  await Promise.all([
    addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('@pnpm.e2e/bar', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest'),
  ])

  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
      },
      devDependencies: {
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/foo': '100.0.0',
      },
      devDependencies: {
        '@pnpm.e2e/bar': '100.0.0',
      },
    },
  ])

  await execPnpm(['-r', 'install'])

  await execPnpm(['-r', 'update', '--latest', '--prod'])

  const manifest1 = await readPackageJsonFromDir(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
  })
  expect(manifest1.devDependencies).toStrictEqual({
    '@pnpm.e2e/foo': '100.0.0',
  })

  const lockfile1 = await projects['project-1'].readLockfile()
  expect(lockfile1.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile1.devDependencies['@pnpm.e2e/foo']).toBe('100.0.0')

  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].has('@pnpm.e2e/foo')

  const manifest2 = await readPackageJsonFromDir(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    '@pnpm.e2e/foo': '100.1.0',
  })
  expect(manifest2.devDependencies).toStrictEqual({
    '@pnpm.e2e/bar': '100.0.0',
  })

  const lockfile2 = await projects['project-2'].readLockfile()
  expect(lockfile2.devDependencies['@pnpm.e2e/bar']).toBe('100.0.0')
  expect(lockfile2.dependencies['@pnpm.e2e/foo']).toBe('100.1.0')

  await projects['project-2'].has('@pnpm.e2e/bar')
  await projects['project-2'].has('@pnpm.e2e/foo')
})

test('recursive update --latest specific dependency on projects that do not share a lockfile', async () => {
  await Promise.all([
    addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('@pnpm.e2e/bar', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/qar', '100.1.0', 'latest'),
  ])

  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        alias: 'npm:@pnpm.e2e/qar@100.0.0',
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
        '@pnpm.e2e/foo': '^100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/bar': '100.0.0',
        '@pnpm.e2e/foo': '^100.0.0',
      },
    },
  ])

  await execPnpm(['-r', 'install'])

  await execPnpm(['-r', 'update', '--latest', '@pnpm.e2e/foo', '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0', 'alias'])

  const manifest1 = await readPackageJsonFromDir(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    alias: 'npm:@pnpm.e2e/qar@^100.1.0', // this might be not correct
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    '@pnpm.e2e/foo': '^100.1.0',
  })

  const lockfile1 = await projects['project-1'].readLockfile()
  expect(lockfile1.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(lockfile1.dependencies['@pnpm.e2e/foo']).toBe('100.1.0')
  expect(lockfile1.dependencies['alias']).toBe('/@pnpm.e2e/qar/100.1.0')

  const manifest2 = await readPackageJsonFromDir(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    '@pnpm.e2e/bar': '100.0.0',
    '@pnpm.e2e/foo': '^100.1.0',
  })

  const lockfile2 = await projects['project-2'].readLockfile()
  expect(lockfile2.dependencies['@pnpm.e2e/bar']).toBe('100.0.0')
  expect(lockfile2.dependencies['@pnpm.e2e/foo']).toBe('100.1.0')
})

test('recursive update --latest on projects with a shared a lockfile', async () => {
  await Promise.all([
    addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('@pnpm.e2e/bar', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest'),
  ])

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/bar': '100.0.0',
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'install'])

  await execPnpm(['recursive', 'update', '--latest'])

  const manifest1 = await readPackageJsonFromDir(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    '@pnpm.e2e/foo': '100.1.0',
  })

  const manifest2 = await readPackageJsonFromDir(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    '@pnpm.e2e/bar': '100.1.0',
    '@pnpm.e2e/foo': '100.1.0',
  })

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // eslint-disable-line
  expect(lockfile.importers['project-1'].dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile.importers['project-1'].dependencies['@pnpm.e2e/foo']).toBe('100.1.0')
  expect(lockfile.importers['project-2'].dependencies['@pnpm.e2e/bar']).toBe('100.1.0')
  expect(lockfile.importers['project-2'].dependencies['@pnpm.e2e/foo']).toBe('100.1.0')
})

test('recursive update --latest --prod on projects with a shared a lockfile', async () => {
  await Promise.all([
    addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('@pnpm.e2e/bar', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest'),
  ])

  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
      },
      devDependencies: {
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/foo': '100.0.0',
      },
      devDependencies: {
        '@pnpm.e2e/bar': '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'install'])

  await execPnpm(['recursive', 'update', '--latest', '--prod'])

  const manifest1 = await readPackageJsonFromDir(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
  })
  expect(manifest1.devDependencies).toStrictEqual({
    '@pnpm.e2e/foo': '100.0.0',
  })

  const manifest2 = await readPackageJsonFromDir(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    '@pnpm.e2e/foo': '100.1.0',
  })
  expect(manifest2.devDependencies).toStrictEqual({
    '@pnpm.e2e/bar': '100.0.0',
  })

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // eslint-disable-line
  expect(lockfile.importers['project-1'].dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile.importers['project-1'].devDependencies['@pnpm.e2e/foo']).toBe('100.0.0')
  expect(lockfile.importers['project-2'].devDependencies['@pnpm.e2e/bar']).toBe('100.0.0')
  expect(lockfile.importers['project-2'].dependencies['@pnpm.e2e/foo']).toBe('100.1.0')

  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].has('@pnpm.e2e/foo')
  await projects['project-2'].has('@pnpm.e2e/foo')
  await projects['project-2'].has('@pnpm.e2e/bar')
})

test('recursive update --latest specific dependency on projects with a shared a lockfile', async () => {
  await Promise.all([
    addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('@pnpm.e2e/bar', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/foo', '100.1.0', 'latest'),
    addDistTag('@pnpm.e2e/qar', '100.1.0', 'latest'),
  ])

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        alias: 'npm:@pnpm.e2e/qar@100.0.0',
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/bar': '100.0.0',
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'install'])

  await execPnpm(['recursive', 'update', '--latest', '@pnpm.e2e/foo', '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0', 'alias'])

  const manifest1 = await readPackageJsonFromDir(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    alias: 'npm:@pnpm.e2e/qar@^100.1.0', // this might be incorrect
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    '@pnpm.e2e/foo': '100.1.0',
  })

  const manifest2 = await readPackageJsonFromDir(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    '@pnpm.e2e/bar': '100.0.0',
    '@pnpm.e2e/foo': '100.1.0',
  })

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // eslint-disable-line
  expect(lockfile.importers['project-1'].dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(lockfile.importers['project-1'].dependencies['@pnpm.e2e/foo']).toBe('100.1.0')
  expect(lockfile.importers['project-1'].dependencies['alias']).toBe('/@pnpm.e2e/qar/100.1.0')
  expect(lockfile.importers['project-2'].dependencies['@pnpm.e2e/bar']).toBe('100.0.0')
  expect(lockfile.importers['project-2'].dependencies['@pnpm.e2e/foo']).toBe('100.1.0')
})

test('deep update', async function () {
  const project = prepare()

  await addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await execPnpm(['add', '@pnpm.e2e/pkg-with-1-dep'])

  await project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm(['update', '--depth', '1'])

  await project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0')
})
