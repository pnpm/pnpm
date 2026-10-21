import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import type { WorkspaceState } from '@pnpm/workspace.state'
import { loadJsonFileSync } from 'load-json-file'

import { recursiveInstallThenUpdateWorkspaceState } from '../src/workspaceStateUpdate.js'
import { DEFAULT_OPTS } from './utils/index.js'

test.each([
  [true, false],
  [false, false],
  [true, true],
  [false, true],
])('an empty workspace install does not record installation proof (shared lockfile: %s, ignored: %s)', async (sharedWorkspaceLockfile, ignored) => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '1.0.0' } },
    { name: 'child', version: '1.0.0', dependencies: { 'is-positive': '1.0.0' } },
  ])
  const { allProjects, allProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await recursiveInstallThenUpdateWorkspaceState(allProjects, [], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    allProjectsGraph,
    lockfileDir: sharedWorkspaceLockfile ? process.cwd() : undefined,
    selectedProjectsGraph: ignored ? allProjectsGraph : {},
    ignoredPackages: ignored ? new Set(Object.keys(allProjectsGraph)) : undefined,
    sharedWorkspaceLockfile,
    workspaceDir: process.cwd(),
  }, 'install')

  expect(fs.existsSync(path.resolve('node_modules/.pnpm-workspace-state-v1.json'))).toBe(false)
})

test.each([
  [true, false],
  [false, false],
  [true, true],
  [false, true],
])('ignored projects receive no installation proof (shared lockfile: %s, ignored root: %s)', async (sharedWorkspaceLockfile, ignoredRoot) => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '1.0.0' } },
    { name: 'child', version: '1.0.0', dependencies: { 'is-positive': '1.0.0' } },
  ])
  const { allProjects, allProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await recursiveInstallThenUpdateWorkspaceState(allProjects, [], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    allProjectsGraph,
    lockfileDir: sharedWorkspaceLockfile ? process.cwd() : undefined,
    selectedProjectsGraph: allProjectsGraph,
    ignoredPackages: new Set([ignoredRoot ? process.cwd() : path.resolve('child')]),
    sharedWorkspaceLockfile,
    workspaceDir: process.cwd(),
  }, 'install')

  const state = loadJsonFileSync<WorkspaceState>('node_modules/.pnpm-workspace-state-v1.json')
  expect(state.filteredInstall).toBe(true)
  expect(Object.keys(state.validation!.manifests)).toStrictEqual([ignoredRoot ? 'child' : '.'])
})
