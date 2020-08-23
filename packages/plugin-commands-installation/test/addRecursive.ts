import { readProjects } from '@pnpm/filter-workspace-packages'
import { Lockfile } from '@pnpm/lockfile-types'
import { add } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import readYamlFile from 'read-yaml-file'
import { DEFAULT_OPTS } from './utils'
import path = require('path')
import test = require('tape')

test('recursive add --save-dev, --save-peer on workspace with multiple lockfiles', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])

  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    saveDev: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-positive@1.0.0'])
  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    savePeer: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-negative@1.0.0'])

  {
    const manifest = (await import(path.resolve('project-1/package.json')))
    t.deepEqual(
      manifest.devDependencies,
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    t.deepEqual(
      manifest.peerDependencies,
      { 'is-negative': '1.0.0' }
    )
    t.deepEqual(
      (await projects['project-1'].readLockfile()).devDependencies,
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
  }

  {
    const manifest = (await import(path.resolve('project-2/package.json')))
    t.deepEqual(
      manifest.devDependencies,
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    t.deepEqual(
      manifest.peerDependencies,
      { 'is-negative': '1.0.0' }
    )
    t.deepEqual(
      (await projects['project-2'].readLockfile()).devDependencies,
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
  }
  t.end()
})

test('recursive add --save-dev, --save-peer on workspace with single lockfile', async (t) => {
  preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])

  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    saveDev: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-positive@1.0.0'])
  await add.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    savePeer: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-negative@1.0.0'])

  {
    const manifest = (await import(path.resolve('project-1/package.json')))
    t.deepEqual(
      manifest.devDependencies,
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    t.deepEqual(
      manifest.peerDependencies,
      { 'is-negative': '1.0.0' }
    )
  }

  {
    const manifest = (await import(path.resolve('project-2/package.json')))
    t.deepEqual(
      manifest.devDependencies,
      { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
    )
    t.deepEqual(
      manifest.peerDependencies,
      { 'is-negative': '1.0.0' }
    )
  }

  const lockfile = await readYamlFile<Lockfile>('./pnpm-lock.yaml')
  t.deepEqual(
    lockfile.importers['project-1'].devDependencies,
    { 'is-positive': '1.0.0', 'is-negative': '1.0.0' }
  )
  t.end()
})
