import prepare, { preparePackages } from '@pnpm/prepare'
import { fromDir as readPackage } from '@pnpm/read-package-json'
import readYamlFile from 'read-yaml-file'
import {
  addDistTag,
  execPnpm,
} from './utils'
import path = require('path')
import writeYamlFile = require('write-yaml-file')

test('update <dep>', async () => {
  const project = prepare()

  await addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest')

  await execPnpm(['install', 'dep-of-pkg-with-1-dep@^100.0.0'])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await execPnpm(['update', 'dep-of-pkg-with-1-dep@latest'])

  await project.storeHas('dep-of-pkg-with-1-dep', '101.0.0')

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['dep-of-pkg-with-1-dep']).toBe('101.0.0')

  const pkg = await readPackage(process.cwd())
  expect(pkg.dependencies?.['dep-of-pkg-with-1-dep']).toBe('^101.0.0')
})

test('update --no-save', async () => {
  await addDistTag('foo', '100.1.0', 'latest')
  const project = prepare({
    dependencies: {
      foo: '^100.0.0',
    },
  })

  await execPnpm(['update', '--no-save'])

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/foo/100.1.0'])

  const pkg = await readPackage(process.cwd())
  expect(pkg.dependencies?.['foo']).toBe('^100.0.0')
})

test('update', async () => {
  await addDistTag('foo', '100.0.0', 'latest')
  const project = prepare({
    dependencies: {
      foo: '^100.0.0',
    },
  })

  await execPnpm(['install', '--lockfile-only'])

  await addDistTag('foo', '100.1.0', 'latest')

  await execPnpm(['update'])

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/foo/100.1.0'])

  const pkg = await readPackage(process.cwd())
  expect(pkg.dependencies?.['foo']).toBe('^100.1.0')
})

test('recursive update --no-save', async () => {
  await addDistTag('foo', '100.1.0', 'latest')
  preparePackages([
    {
      location: 'project',
      package: {
        dependencies: {
          foo: '^100.0.0',
        },
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'update', '--no-save'])

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // eslint-disable-line
  expect(lockfile.packages).toHaveProperty(['/foo/100.1.0'])

  const pkg = await readPackage(path.resolve('project'))
  expect(pkg.dependencies?.['foo']).toBe('^100.0.0')
})

test('recursive update', async () => {
  await addDistTag('foo', '100.1.0', 'latest')
  preparePackages([
    {
      location: 'project',
      package: {
        dependencies: {
          foo: '^100.0.0',
        },
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'update'])

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // eslint-disable-line
  expect(lockfile.packages).toHaveProperty(['/foo/100.1.0'])

  const pkg = await readPackage(path.resolve('project'))
  expect(pkg.dependencies?.['foo']).toBe('^100.1.0')
})

test('recursive update --no-shared-workspace-lockfile', async function () {
  await addDistTag('foo', '100.1.0', 'latest')
  const projects = preparePackages([
    {
      location: 'project',
      package: {
        name: 'project',

        dependencies: {
          foo: '^100.0.0',
        },
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'update', '--no-shared-workspace-lockfile'])

  const lockfile = await projects['project'].readLockfile()
  expect(lockfile.packages).toHaveProperty(['/foo/100.1.0'])

  const pkg = await readPackage(path.resolve('project'))
  expect(pkg.dependencies?.['foo']).toBe('^100.1.0')
})

test('update should not install the dependency if it is not present already', async () => {
  const project = prepare()

  let err!: Error
  try {
    await execPnpm(['update', 'is-positive'])
  } catch (_err) {
    err = _err
  }
  expect(err).toBeTruthy()

  await project.hasNot('is-positive')
})

test('update --latest', async function () {
  const project = prepare()

  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('qar', '100.1.0', 'latest'),
  ])

  await execPnpm(['add', 'dep-of-pkg-with-1-dep@^100.0.0', 'bar@^100.0.0', 'alias@npm:qar@^100.0.0', 'kevva/is-negative'])

  await execPnpm(['update', '--latest'])

  await project.storeHas('dep-of-pkg-with-1-dep', '101.0.0')

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile.dependencies['bar']).toBe('100.1.0')
  expect(lockfile.dependencies['alias']).toBe('/qar/100.1.0')

  const pkg = await readPackage(process.cwd())
  expect(pkg.dependencies?.['dep-of-pkg-with-1-dep']).toBe('^101.0.0')
  expect(pkg.dependencies?.['bar']).toBe('^100.1.0')
  expect(pkg.dependencies?.['alias']).toBe('npm:qar@^100.1.0')
  expect(pkg.dependencies?.['is-negative']).toBe('github:kevva/is-negative')
})

test('update --latest --save-exact', async function () {
  const project = prepare()

  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('qar', '100.1.0', 'latest'),
  ])

  await execPnpm(['install', 'dep-of-pkg-with-1-dep@100.0.0', 'bar@100.0.0', 'alias@npm:qar@100.0.0', 'kevva/is-negative'])

  await execPnpm(['update', '--latest', '--save-exact'])

  await project.storeHas('dep-of-pkg-with-1-dep', '101.0.0')

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile.dependencies['bar']).toBe('100.1.0')
  expect(lockfile.dependencies['alias']).toBe('/qar/100.1.0')

  const pkg = await readPackage(process.cwd())
  expect(pkg.dependencies?.['dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(pkg.dependencies?.['bar']).toBe('100.1.0')
  expect(pkg.dependencies?.['alias']).toBe('npm:qar@100.1.0')
  expect(pkg.dependencies?.['is-negative']).toBe('github:kevva/is-negative')
})

test('update --latest specific dependency', async function () {
  const project = prepare()

  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('qar', '100.1.0', 'latest'),
  ])

  await execPnpm(['add', 'dep-of-pkg-with-1-dep@100.0.0', 'bar@^100.0.0', 'foo@100.1.0', 'alias@npm:qar@^100.0.0', 'kevva/is-negative'])

  await execPnpm(['update', '-L', 'bar', 'foo@100.0.0', 'alias', 'is-negative'])

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies['dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(lockfile.dependencies['bar']).toBe('100.1.0')
  expect(lockfile.dependencies['foo']).toBe('100.0.0')
  expect(lockfile.dependencies['alias']).toBe('/qar/100.1.0')

  const pkg = await readPackage(process.cwd())
  expect(pkg.dependencies?.['dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(pkg.dependencies?.['bar']).toBe('^100.1.0')
  expect(pkg.dependencies?.['foo']).toBe('100.0.0')
  expect(pkg.dependencies?.['alias']).toBe('npm:qar@^100.1.0')
  expect(pkg.dependencies?.['is-negative']).toBe('github:kevva/is-negative')
})

test('update --latest --prod', async function () {
  const project = prepare()

  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
  ])

  await execPnpm(['add', '-D', 'dep-of-pkg-with-1-dep@100.0.0'])
  await execPnpm(['add', '-P', 'bar@^100.0.0'])

  await execPnpm(['update', '--latest', '--prod'])

  const lockfile = await project.readLockfile()
  expect(lockfile.devDependencies['dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(lockfile.dependencies['bar']).toBe('100.1.0')

  const pkg = await readPackage(process.cwd())
  expect(pkg.devDependencies?.['dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(pkg.dependencies?.['bar']).toBe('^100.1.0')

  await project.has('dep-of-pkg-with-1-dep') // not pruned
})

test('recursive update --latest on projects that do not share a lockfile', async () => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
  ])

  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'dep-of-pkg-with-1-dep': '100.0.0',
        foo: '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        bar: '100.0.0',
        foo: '100.0.0',
      },
    },
  ])

  await execPnpm(['recursive', 'install'])

  await execPnpm(['recursive', 'update', '--latest'])

  const manifest1 = await readPackage(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    'dep-of-pkg-with-1-dep': '101.0.0',
    foo: '100.1.0',
  })

  const lockfile1 = await projects['project-1'].readLockfile()
  expect(lockfile1.dependencies['dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile1.dependencies['foo']).toBe('100.1.0')

  const manifest2 = await readPackage(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    bar: '100.1.0',
    foo: '100.1.0',
  })

  const lockfile2 = await projects['project-2'].readLockfile()
  expect(lockfile2.dependencies['bar']).toBe('100.1.0')
  expect(lockfile2.dependencies['foo']).toBe('100.1.0')
})

test('recursive update --latest --prod on projects that do not share a lockfile', async () => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
  ])

  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'dep-of-pkg-with-1-dep': '100.0.0',
      },
      devDependencies: {
        foo: '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        foo: '100.0.0',
      },
      devDependencies: {
        bar: '100.0.0',
      },
    },
  ])

  await execPnpm(['recursive', 'install'])

  await execPnpm(['recursive', 'update', '--latest', '--prod'])

  const manifest1 = await readPackage(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    'dep-of-pkg-with-1-dep': '101.0.0',
  })
  expect(manifest1.devDependencies).toStrictEqual({
    foo: '100.0.0',
  })

  const lockfile1 = await projects['project-1'].readLockfile()
  expect(lockfile1.dependencies['dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile1.devDependencies['foo']).toBe('100.0.0')

  await projects['project-1'].has('dep-of-pkg-with-1-dep')
  await projects['project-1'].has('foo')

  const manifest2 = await readPackage(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    foo: '100.1.0',
  })
  expect(manifest2.devDependencies).toStrictEqual({
    bar: '100.0.0',
  })

  const lockfile2 = await projects['project-2'].readLockfile()
  expect(lockfile2.devDependencies['bar']).toBe('100.0.0')
  expect(lockfile2.dependencies['foo']).toBe('100.1.0')

  await projects['project-2'].has('bar')
  await projects['project-2'].has('foo')
})

test('recursive update --latest specific dependency on projects that do not share a lockfile', async () => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('qar', '100.1.0', 'latest'),
  ])

  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        alias: 'npm:qar@100.0.0',
        'dep-of-pkg-with-1-dep': '101.0.0',
        foo: '^100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        bar: '100.0.0',
        foo: '^100.0.0',
      },
    },
  ])

  await execPnpm(['recursive', 'install'])

  await execPnpm(['recursive', 'update', '--latest', 'foo', 'dep-of-pkg-with-1-dep@100.0.0', 'alias'])

  const manifest1 = await readPackage(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    alias: 'npm:qar@100.1.0',
    'dep-of-pkg-with-1-dep': '100.0.0',
    foo: '^100.1.0',
  })

  const lockfile1 = await projects['project-1'].readLockfile()
  expect(lockfile1.dependencies['dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(lockfile1.dependencies['foo']).toBe('100.1.0')
  expect(lockfile1.dependencies['alias']).toBe('/qar/100.1.0')

  const manifest2 = await readPackage(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    bar: '100.0.0',
    foo: '^100.1.0',
  })

  const lockfile2 = await projects['project-2'].readLockfile()
  expect(lockfile2.dependencies['bar']).toBe('100.0.0')
  expect(lockfile2.dependencies['foo']).toBe('100.1.0')
})

test('recursive update --latest on projects with a shared a lockfile', async () => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
  ])

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'dep-of-pkg-with-1-dep': '100.0.0',
        foo: '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        bar: '100.0.0',
        foo: '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'install'])

  await execPnpm(['recursive', 'update', '--latest'])

  const manifest1 = await readPackage(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    'dep-of-pkg-with-1-dep': '101.0.0',
    foo: '100.1.0',
  })

  const manifest2 = await readPackage(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    bar: '100.1.0',
    foo: '100.1.0',
  })

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // eslint-disable-line
  expect(lockfile.importers['project-1'].dependencies['dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile.importers['project-1'].dependencies['foo']).toBe('100.1.0')
  expect(lockfile.importers['project-2'].dependencies['bar']).toBe('100.1.0')
  expect(lockfile.importers['project-2'].dependencies['foo']).toBe('100.1.0')
})

test('recursive update --latest --prod on projects with a shared a lockfile', async () => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
  ])

  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'dep-of-pkg-with-1-dep': '100.0.0',
      },
      devDependencies: {
        foo: '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        foo: '100.0.0',
      },
      devDependencies: {
        bar: '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'install'])

  await execPnpm(['recursive', 'update', '--latest', '--prod'])

  const manifest1 = await readPackage(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    'dep-of-pkg-with-1-dep': '101.0.0',
  })
  expect(manifest1.devDependencies).toStrictEqual({
    foo: '100.0.0',
  })

  const manifest2 = await readPackage(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    foo: '100.1.0',
  })
  expect(manifest2.devDependencies).toStrictEqual({
    bar: '100.0.0',
  })

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // eslint-disable-line
  expect(lockfile.importers['project-1'].dependencies['dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile.importers['project-1'].devDependencies['foo']).toBe('100.0.0')
  expect(lockfile.importers['project-2'].devDependencies['bar']).toBe('100.0.0')
  expect(lockfile.importers['project-2'].dependencies['foo']).toBe('100.1.0')

  await projects['project-1'].has('dep-of-pkg-with-1-dep')
  await projects['project-1'].has('foo')
  await projects['project-2'].has('foo')
  await projects['project-2'].has('bar')
})

test('recursive update --latest specific dependency on projects with a shared a lockfile', async () => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('qar', '100.1.0', 'latest'),
  ])

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        alias: 'npm:qar@100.0.0',
        'dep-of-pkg-with-1-dep': '101.0.0',
        foo: '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        bar: '100.0.0',
        foo: '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['recursive', 'install'])

  await execPnpm(['recursive', 'update', '--latest', 'foo', 'dep-of-pkg-with-1-dep@100.0.0', 'alias'])

  const manifest1 = await readPackage(path.resolve('project-1'))
  expect(manifest1.dependencies).toStrictEqual({
    alias: 'npm:qar@100.1.0',
    'dep-of-pkg-with-1-dep': '100.0.0',
    foo: '100.1.0',
  })

  const manifest2 = await readPackage(path.resolve('project-2'))
  expect(manifest2.dependencies).toStrictEqual({
    bar: '100.0.0',
    foo: '100.1.0',
  })

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // eslint-disable-line
  expect(lockfile.importers['project-1'].dependencies['dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(lockfile.importers['project-1'].dependencies['foo']).toBe('100.1.0')
  expect(lockfile.importers['project-1'].dependencies['alias']).toBe('/qar/100.1.0')
  expect(lockfile.importers['project-2'].dependencies['bar']).toBe('100.0.0')
  expect(lockfile.importers['project-2'].dependencies['foo']).toBe('100.1.0')
})

test('deep update', async function () {
  const project = prepare()

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await execPnpm(['add', 'pkg-with-1-dep'])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm(['update', '--depth', '1'])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})
