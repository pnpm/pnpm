import assertProject from '@pnpm/assert-project'
import { preparePackages } from '@pnpm/prepare'
import path = require('path')
import readPkg = require('read-pkg')
import sinon = require('sinon')
import { addDependenciesToPackage, install, MutatedImporter, mutateModules } from 'supi'
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
      mutation: 'install',
      prefix: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      prefix: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ shrinkwrapOnly: true }))

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
      mutation: 'install',
      prefix: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      prefix: path.resolve('project-2'),
    },
  ], await testDefaults())

  await addDependenciesToPackage(['is-positive@2'], await testDefaults({
    prefix: path.resolve('project-1'),
    shrinkwrapDirectory: process.cwd(),
  }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  const rootNodeModules = assertProject(t, process.cwd())
  await rootNodeModules.has('.localhost+4873/is-positive/2.0.0')
  await rootNodeModules.hasNot('.localhost+4873/is-positive/1.0.0')
  await rootNodeModules.has('.localhost+4873/is-negative/1.0.0')
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
      mutation: 'install',
      prefix: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      prefix: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults())

  await addDependenciesToPackage(['is-positive@2'], await testDefaults({
    prefix: path.resolve('project-1'),
    shrinkwrapDirectory: process.cwd(),
    shrinkwrapOnly: true,
  }))
  await mutateModules(importers.slice(0, 1), await testDefaults({ frozenShrinkwrap: true }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  const rootNodeModules = assertProject(t, process.cwd())
  await rootNodeModules.has('.localhost+4873/is-positive/2.0.0')
  await rootNodeModules.hasNot('.localhost+4873/is-positive/1.0.0')
  await rootNodeModules.has('.localhost+4873/is-negative/1.0.0')
})

test('adding a new dev dependency to project that uses a shared shrinkwrap', async (t) => {
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
      mutation: 'install',
      prefix: path.resolve('project-1'),
    },
  ], await testDefaults())
  await addDependenciesToPackage(['is-negative@1.0.0'], await testDefaults({ prefix: path.resolve('project-1'), targetDependenciesField: 'devDependencies' }))

  const pkg = await readPkg({ cwd: 'project-1' })

  t.deepEqual(pkg.dependencies, { 'is-positive': '1.0.0' }, 'prod deps unchanged in package.json')
  t.deepEqual(pkg.devDependencies, { 'is-negative': '^1.0.0' }, 'dev deps have a new dependency in package.json')
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
      mutation: 'install',
      prefix: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      prefix: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ shrinkwrapOnly: true }))

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
