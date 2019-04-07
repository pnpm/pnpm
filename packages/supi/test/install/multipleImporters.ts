import assertProject from '@pnpm/assert-project'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { readCurrentLockfile } from '@pnpm/lockfile-file'
import { preparePackages } from '@pnpm/prepare'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import path = require('path')
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  MutatedImporter,
  mutateModules,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('install only the dependencies of the specified importer', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const importers: MutatedImporter[] = [
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ lockfileOnly: true }))

  await mutateModules(importers.slice(0, 1), await testDefaults())

  await projects['project-1'].has('is-positive')
  await projects['project-2'].hasNot('is-negative')

  const rootNodeModules = assertProject(t, process.cwd())
  await rootNodeModules.has('.localhost+4873/is-positive/1.0.0')
  await rootNodeModules.hasNot('.localhost+4873/is-negative/1.0.0')
})

test('dependencies of other importers are not pruned when installing for a subset of importers', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await mutateModules([
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: path.resolve('project-2'),
    },
  ], await testDefaults())

  await addDependenciesToPackage(['is-positive@2'], await testDefaults({
    lockfileDirectory: process.cwd(),
    prefix: path.resolve('project-1'),
  }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  const rootNodeModules = assertProject(t, process.cwd())
  await rootNodeModules.has('.localhost+4873/is-positive/2.0.0')
  await rootNodeModules.hasNot('.localhost+4873/is-positive/1.0.0')
  await rootNodeModules.has('.localhost+4873/is-negative/1.0.0')

  const lockfile = await rootNodeModules.loadCurrentLockfile()
  t.deepEqual(Object.keys(lockfile.packages), [
    '/is-negative/1.0.0',
    '/is-positive/2.0.0',
  ], `packages of importer that was not selected by last installation are not removed from current ${WANTED_LOCKFILE}`)
})

test('dependencies of other importers are not pruned when (headless) installing for a subset of importers', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const importers: MutatedImporter[] = [
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults())

  await addDependenciesToPackage(['is-positive@2'], await testDefaults({
    lockfileDirectory: process.cwd(),
    lockfileOnly: true,
    prefix: path.resolve('project-1'),
  }))
  await mutateModules(importers.slice(0, 1), await testDefaults({ frozenLockfile: true }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  const rootNodeModules = assertProject(t, process.cwd())
  await rootNodeModules.has('.localhost+4873/is-positive/2.0.0')
  await rootNodeModules.hasNot('.localhost+4873/is-positive/1.0.0')
  await rootNodeModules.has('.localhost+4873/is-negative/1.0.0')
})

test('adding a new dev dependency to project that uses a shared lockfile', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  await mutateModules([
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: path.resolve('project-1'),
    },
  ], await testDefaults())
  await addDependenciesToPackage(['is-negative@1.0.0'], await testDefaults({ prefix: path.resolve('project-1'), targetDependenciesField: 'devDependencies' }))

  const pkg = await readPackageJsonFromDir(path.resolve('project-1'))

  t.deepEqual(pkg.dependencies, { 'is-positive': '1.0.0' }, 'prod deps unchanged in package.json')
  t.deepEqual(pkg.devDependencies, { 'is-negative': '1.0.0' }, 'dev deps have a new dependency in package.json')
})

test('headless install is used when package link to another package in the workspace', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': 'file:../project-2',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const importers: MutatedImporter[] = [
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ lockfileOnly: true }))

  const reporter = sinon.spy()
  await mutateModules(importers.slice(0, 1), await testDefaults({ reporter }))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: 'Performing headless installation',
    name: 'pnpm',
  }), 'start of headless installation logged')

  await projects['project-1'].has('is-positive')
  await projects['project-1'].has('project-2')
  await projects['project-2'].hasNot('is-negative')
})

test('current lockfile contains only installed dependencies when adding a new importer to workspace with shared lockfile', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await mutateModules([
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: path.resolve('project-1'),
    },
  ], await testDefaults({ lockfileOnly: true }))

  await mutateModules([
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: path.resolve('project-2'),
    },
  ], await testDefaults())

  const currentLockfile = await readCurrentLockfile(process.cwd(), { ignoreIncompatible: false })

  t.deepEqual(Object.keys(currentLockfile && currentLockfile.packages || {}), ['/is-negative/1.0.0'])
})
