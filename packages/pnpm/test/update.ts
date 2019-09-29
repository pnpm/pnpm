import prepare, { preparePackages } from '@pnpm/prepare'
import { fromDir as readPackage } from '@pnpm/read-package-json'
import path = require('path')
import readYamlFile from 'read-yaml-file'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import {
  addDistTag,
  execPnpm,
} from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('update', async function (t: tape.Test) {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest')

  await execPnpm('install', 'dep-of-pkg-with-1-dep@100.0.0')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await execPnpm('update', 'dep-of-pkg-with-1-dep@latest')

  await project.storeHas('dep-of-pkg-with-1-dep', '101.0.0')

  const lockfile = await project.readLockfile()
  t.equal(lockfile.dependencies['dep-of-pkg-with-1-dep'], '101.0.0')

  const pkg = await readPackage(process.cwd())
  t.equal(pkg.dependencies && pkg.dependencies['dep-of-pkg-with-1-dep'], '^101.0.0')
})

test('update should not install the dependency if it is not present already', async function (t: tape.Test) {
  const project = prepare(t)

  await execPnpm('update', 'is-positive')

  project.hasNot('is-positive')
})

test('update --latest', async function (t: tape.Test) {
  const project = prepare(t)

  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('qar', '100.1.0', 'latest'),
  ])

  await execPnpm('install', 'dep-of-pkg-with-1-dep@100.0.0', 'bar@100.0.0', 'alias@npm:qar@100.0.0', 'kevva/is-negative')

  await execPnpm('update', '--latest')

  await project.storeHas('dep-of-pkg-with-1-dep', '101.0.0')

  const lockfile = await project.readLockfile()
  t.equal(lockfile.dependencies['dep-of-pkg-with-1-dep'], '101.0.0')
  t.equal(lockfile.dependencies['bar'], '100.1.0')
  t.equal(lockfile.dependencies['alias'], '/qar/100.1.0')

  const pkg = await readPackage(process.cwd())
  t.equal(pkg.dependencies && pkg.dependencies['dep-of-pkg-with-1-dep'], '^101.0.0')
  t.equal(pkg.dependencies && pkg.dependencies['bar'], '^100.1.0')
  t.equal(pkg.dependencies && pkg.dependencies['alias'], 'npm:qar@^100.1.0')
  t.equal(pkg.dependencies && pkg.dependencies['is-negative'], 'github:kevva/is-negative', 'do not touch non-npm hosted package')
})

test('update --latest --save-exact', async function (t: tape.Test) {
  const project = prepare(t)

  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('qar', '100.1.0', 'latest'),
  ])

  await execPnpm('install', 'dep-of-pkg-with-1-dep@100.0.0', 'bar@100.0.0', 'alias@npm:qar@100.0.0', 'kevva/is-negative')

  await execPnpm('update', '--latest', '--save-exact')

  await project.storeHas('dep-of-pkg-with-1-dep', '101.0.0')

  const lockfile = await project.readLockfile()
  t.equal(lockfile.dependencies['dep-of-pkg-with-1-dep'], '101.0.0')
  t.equal(lockfile.dependencies['bar'], '100.1.0')
  t.equal(lockfile.dependencies['alias'], '/qar/100.1.0')

  const pkg = await readPackage(process.cwd())
  t.equal(pkg.dependencies && pkg.dependencies['dep-of-pkg-with-1-dep'], '101.0.0')
  t.equal(pkg.dependencies && pkg.dependencies['bar'], '100.1.0')
  t.equal(pkg.dependencies && pkg.dependencies['alias'], 'npm:qar@100.1.0')
  t.equal(pkg.dependencies && pkg.dependencies['is-negative'], 'github:kevva/is-negative', 'do not touch non-npm hosted package')
})

test('update --latest specific dependency', async function (t: tape.Test) {
  const project = prepare(t)

  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('qar', '100.1.0', 'latest'),
  ])

  await execPnpm('install', 'dep-of-pkg-with-1-dep@100.0.0', 'bar@100.0.0', 'foo@100.1.0', 'alias@npm:qar@100.0.0', 'kevva/is-negative')

  await execPnpm('update', '-L', 'bar', 'foo@100.0.0', 'alias', 'is-negative')

  const lockfile = await project.readLockfile()
  t.equal(lockfile.dependencies['dep-of-pkg-with-1-dep'], '100.0.0')
  t.equal(lockfile.dependencies['bar'], '100.1.0')
  t.equal(lockfile.dependencies['foo'], '100.0.0')
  t.equal(lockfile.dependencies['alias'], '/qar/100.1.0')

  const pkg = await readPackage(process.cwd())
  t.equal(pkg.dependencies && pkg.dependencies['dep-of-pkg-with-1-dep'], '100.0.0')
  t.equal(pkg.dependencies && pkg.dependencies['bar'], '^100.1.0')
  t.equal(pkg.dependencies && pkg.dependencies['foo'], '100.0.0')
  t.equal(pkg.dependencies && pkg.dependencies['alias'], 'npm:qar@^100.1.0')
  t.equal(pkg.dependencies && pkg.dependencies['is-negative'], 'github:kevva/is-negative', 'do not touch non-npm hosted package')
})

test('update --latest --prod', async function (t: tape.Test) {
  const project = prepare(t)

  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
  ])

  await execPnpm('install', '-D', 'dep-of-pkg-with-1-dep@100.0.0')
  await execPnpm('install', '-P', 'bar@100.0.0')

  await execPnpm('update', '--latest', '--prod')

  const lockfile = await project.readLockfile()
  t.equal(lockfile.devDependencies['dep-of-pkg-with-1-dep'], '100.0.0')
  t.equal(lockfile.dependencies['bar'], '100.1.0')

  const pkg = await readPackage(process.cwd())
  t.equal(pkg.devDependencies && pkg.devDependencies['dep-of-pkg-with-1-dep'], '100.0.0')
  t.equal(pkg.dependencies && pkg.dependencies['bar'], '^100.1.0')

  await project.has('dep-of-pkg-with-1-dep') // not pruned
})

test('recursive update --latest on projects that do not share a lockfile', async (t: tape.Test) => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
  ])

  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'dep-of-pkg-with-1-dep': '100.0.0',
        'foo': '100.0.0'
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'bar': '100.0.0',
        'foo': '100.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'install')

  await execPnpm('recursive', 'update', '--latest')

  const manifest1 = await readPackage(path.resolve('project-1'))
  t.deepEqual(manifest1.dependencies, {
    'dep-of-pkg-with-1-dep': '^101.0.0',
    'foo': '^100.1.0'
  })

  const lockfile1 = await projects['project-1'].readLockfile()
  t.equal(lockfile1.dependencies['dep-of-pkg-with-1-dep'], '101.0.0')
  t.equal(lockfile1.dependencies['foo'], '100.1.0')

  const manifest2 = await readPackage(path.resolve('project-2'))
  t.deepEqual(manifest2.dependencies, {
    'bar': '^100.1.0',
    'foo': '^100.1.0',
  })

  const lockfile2 = await projects['project-2'].readLockfile()
  t.equal(lockfile2.dependencies['bar'], '100.1.0')
  t.equal(lockfile2.dependencies['foo'], '100.1.0')
})

test('recursive update --latest --prod on projects that do not share a lockfile', async (t: tape.Test) => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
  ])

  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'dep-of-pkg-with-1-dep': '100.0.0',
      },
      devDependencies: {
        'foo': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'foo': '100.0.0',
      },
      devDependencies: {
        'bar': '100.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'install')

  await execPnpm('recursive', 'update', '--latest', '--prod')

  const manifest1 = await readPackage(path.resolve('project-1'))
  t.deepEqual(manifest1.dependencies, {
    'dep-of-pkg-with-1-dep': '^101.0.0',
  })
  t.deepEqual(manifest1.devDependencies, {
    'foo': '100.0.0'
  })

  const lockfile1 = await projects['project-1'].readLockfile()
  t.equal(lockfile1.dependencies['dep-of-pkg-with-1-dep'], '101.0.0')
  t.equal(lockfile1.devDependencies['foo'], '100.0.0')

  await projects['project-1'].has('dep-of-pkg-with-1-dep')
  await projects['project-1'].has('foo')

  const manifest2 = await readPackage(path.resolve('project-2'))
  t.deepEqual(manifest2.dependencies, {
    'foo': '^100.1.0',
  })
  t.deepEqual(manifest2.devDependencies, {
    'bar': '100.0.0',
  })

  const lockfile2 = await projects['project-2'].readLockfile()
  t.equal(lockfile2.devDependencies['bar'], '100.0.0')
  t.equal(lockfile2.dependencies['foo'], '100.1.0')

  await projects['project-2'].has('bar')
  await projects['project-2'].has('foo')
})

test('recursive update --latest specific dependency on projects that do not share a lockfile', async (t: tape.Test) => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('qar', '100.1.0', 'latest'),
  ])

  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'alias': 'npm:qar@100.0.0',
        'dep-of-pkg-with-1-dep': '101.0.0',
        'foo': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'bar': '100.0.0',
        'foo': '100.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'install')

  await execPnpm('recursive', 'update', '--latest', 'foo', 'dep-of-pkg-with-1-dep@100.0.0', 'alias')

  const manifest1 = await readPackage(path.resolve('project-1'))
  t.deepEqual(manifest1.dependencies, {
    'alias': 'npm:qar@^100.1.0',
    'dep-of-pkg-with-1-dep': '100.0.0',
    'foo': '^100.1.0',
  })

  const lockfile1 = await projects['project-1'].readLockfile()
  t.equal(lockfile1.dependencies['dep-of-pkg-with-1-dep'], '100.0.0')
  t.equal(lockfile1.dependencies['foo'], '100.1.0')
  t.equal(lockfile1.dependencies['alias'], '/qar/100.1.0')

  const manifest2 = await readPackage(path.resolve('project-2'))
  t.deepEqual(manifest2.dependencies, {
    'bar': '100.0.0',
    'foo': '^100.1.0',
  })

  const lockfile2 = await projects['project-2'].readLockfile()
  t.equal(lockfile2.dependencies['bar'], '100.0.0')
  t.equal(lockfile2.dependencies['foo'], '100.1.0')
})

test('recursive update --latest on projects with a shared a lockfile', async (t: tape.Test) => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
  ])

  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'dep-of-pkg-with-1-dep': '100.0.0',
        'foo': '100.0.0'
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'bar': '100.0.0',
        'foo': '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm('recursive', 'install')

  await execPnpm('recursive', 'update', '--latest')

  const manifest1 = await readPackage(path.resolve('project-1'))
  t.deepEqual(manifest1.dependencies, {
    'dep-of-pkg-with-1-dep': '^101.0.0',
    'foo': '^100.1.0'
  })

  const manifest2 = await readPackage(path.resolve('project-2'))
  t.deepEqual(manifest2.dependencies, {
    'bar': '^100.1.0',
    'foo': '^100.1.0',
  })

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // tslint:disable-line
  t.equal(lockfile.importers['project-1'].dependencies['dep-of-pkg-with-1-dep'], '101.0.0')
  t.equal(lockfile.importers['project-1'].dependencies['foo'], '100.1.0')
  t.equal(lockfile.importers['project-2'].dependencies['bar'], '100.1.0')
  t.equal(lockfile.importers['project-2'].dependencies['foo'], '100.1.0')
})

test('recursive update --latest --prod on projects with a shared a lockfile', async (t: tape.Test) => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
  ])

  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'dep-of-pkg-with-1-dep': '100.0.0',
      },
      devDependencies: {
        'foo': '100.0.0',
      }
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'foo': '100.0.0',
      },
      devDependencies: {
        'bar': '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm('recursive', 'install')

  await execPnpm('recursive', 'update', '--latest', '--prod')

  const manifest1 = await readPackage(path.resolve('project-1'))
  t.deepEqual(manifest1.dependencies, {
    'dep-of-pkg-with-1-dep': '^101.0.0',
  })
  t.deepEqual(manifest1.devDependencies, {
    'foo': '100.0.0'
  })

  const manifest2 = await readPackage(path.resolve('project-2'))
  t.deepEqual(manifest2.dependencies, {
    'foo': '^100.1.0',
  })
  t.deepEqual(manifest2.devDependencies, {
    'bar': '100.0.0',
  })

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // tslint:disable-line
  t.equal(lockfile.importers['project-1'].dependencies['dep-of-pkg-with-1-dep'], '101.0.0')
  t.equal(lockfile.importers['project-1'].devDependencies['foo'], '100.0.0')
  t.equal(lockfile.importers['project-2'].devDependencies['bar'], '100.0.0')
  t.equal(lockfile.importers['project-2'].dependencies['foo'], '100.1.0')

  await projects['project-1'].has('dep-of-pkg-with-1-dep')
  await projects['project-1'].has('foo')
  await projects['project-2'].has('foo')
  await projects['project-2'].has('bar')
})

test('recursive update --latest specific dependency on projects with a shared a lockfile', async (t: tape.Test) => {
  await Promise.all([
    addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('qar', '100.1.0', 'latest'),
  ])

  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'alias': 'npm:qar@100.0.0',
        'dep-of-pkg-with-1-dep': '101.0.0',
        'foo': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'bar': '100.0.0',
        'foo': '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm('recursive', 'install')

  await execPnpm('recursive', 'update', '--latest', 'foo', 'dep-of-pkg-with-1-dep@100.0.0', 'alias')

  const manifest1 = await readPackage(path.resolve('project-1'))
  t.deepEqual(manifest1.dependencies, {
    'alias': 'npm:qar@^100.1.0',
    'dep-of-pkg-with-1-dep': '100.0.0',
    'foo': '^100.1.0',
  })

  const manifest2 = await readPackage(path.resolve('project-2'))
  t.deepEqual(manifest2.dependencies, {
    'bar': '100.0.0',
    'foo': '^100.1.0',
  })

  const lockfile = await readYamlFile<any>('pnpm-lock.yaml') // tslint:disable-line
  t.equal(lockfile.importers['project-1'].dependencies['dep-of-pkg-with-1-dep'], '100.0.0')
  t.equal(lockfile.importers['project-1'].dependencies['foo'], '100.1.0')
  t.equal(lockfile.importers['project-1'].dependencies['alias'], '/qar/100.1.0')
  t.equal(lockfile.importers['project-2'].dependencies['bar'], '100.0.0')
  t.equal(lockfile.importers['project-2'].dependencies['foo'], '100.1.0')
})

test('deep update', async function (t: tape.Test) {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await execPnpm('add', 'pkg-with-1-dep')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm('update', '--depth', '1')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})
