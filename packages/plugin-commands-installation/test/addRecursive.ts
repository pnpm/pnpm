import { readProjects } from '@pnpm/filter-workspace-packages'
import { add } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import path = require('path')
import test = require('tape')
import { DEFAULT_OPTS } from './utils'

test('recursive add --save-dev on workspace with multiple lockfiles', async (t) => {
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

  await add.handler(['is-positive@1.0.0'], {
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    saveDev: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  t.deepEqual(
    (await import(path.resolve('project-1/package.json'))).devDependencies,
    { 'is-positive': '1.0.0' },
  )
  t.deepEqual(
    (await projects['project-1'].readLockfile()).devDependencies,
    { 'is-positive': '1.0.0' },
  )

  t.deepEqual(
    (await import(path.resolve('project-2/package.json'))).devDependencies,
    { 'is-positive': '1.0.0' },
  )
  t.deepEqual(
    (await projects['project-2'].readLockfile()).devDependencies,
    { 'is-positive': '1.0.0' },
  )
  t.end()
})
