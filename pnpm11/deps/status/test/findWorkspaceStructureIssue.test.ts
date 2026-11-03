import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'
import type { ProjectsList } from '@pnpm/workspace.state'

import type { WorkspaceDepsStatusContext } from '../src/checkWorkspaceDepsStatus.js'
import { findWorkspaceStructureIssue } from '../src/findWorkspaceStructureIssue.js'

const workspaceDir = path.resolve('workspace')
const pkgDir = path.join(workspaceDir, 'packages/a')

// https://github.com/pnpm/pnpm/issues/17560
test('a version bump of a lone project is not a structure change', () => {
  const allProjects: ProjectsList = [
    { rootDir: workspaceDir as ProjectRootDir, manifest: { name: 'root', version: '1.0.1' } },
  ]
  expect(findWorkspaceStructureIssue(context(allProjects))).toBeUndefined()
})

test('a version bump of a lone project is a structure change under deep workspace linking', () => {
  const allProjects: ProjectsList = [
    { rootDir: workspaceDir as ProjectRootDir, manifest: { name: 'root', version: '1.0.1' } },
  ]
  expect(findWorkspaceStructureIssue(context(allProjects, { linkWorkspacePackages: 'deep' })))
    .toBe(STRUCTURE_CHANGED)
})

// https://github.com/pnpm/tasks/issues/246
test('a version bump of a workspace project nothing depends on is not a structure change', () => {
  const root = { name: 'root', version: '1.0.0', dependencies: { library: '1.0.0', '@scope/a': '1.0.0' } }
  expect(findWorkspaceStructureIssue(context(bumpA(root)))).toBeUndefined()
})

test.each<Partial<ProjectManifest>>([
  { dependencies: { a: 'workspace:*' } },
  { devDependencies: { a: '1.0.0' } },
  { optionalDependencies: { a: '1.0.0' } },
  { peerDependencies: { a: '1.0.0' } },
  { dependencies: { renamed: 'npm:a@^1.0.0' } },
  { dependencies: { renamed: 'npm:a' } },
  { dependencies: { renamed: 'workspace:a@*' } },
])('a version bump of a workspace project that %j names is a structure change', (dependencies) => {
  const root = { name: 'root', version: '1.0.0', ...dependencies }
  expect(findWorkspaceStructureIssue(context(bumpA(root)))).toBe(STRUCTURE_CHANGED)
})

test.each([
  'workspace:../a',
  'file:../a',
  'link:../a',
  '../a',
])('every version bump in a workspace is a structure change when a dependency is the local path %s', (spec) => {
  const root = { name: 'root', version: '1.0.0', dependencies: { renamed: spec } }
  expect(findWorkspaceStructureIssue(context(bumpA(root)))).toBe(STRUCTURE_CHANGED)
})

test('a version bump of a workspace project a catalog entry names is a structure change', () => {
  const root = { name: 'root', version: '1.0.0', dependencies: { renamed: 'catalog:' } }
  const catalogs = { default: { renamed: 'npm:a@^1.0.0' } }
  expect(findWorkspaceStructureIssue(context(bumpA(root), { catalogs }, { catalogs })))
    .toBe(STRUCTURE_CHANGED)
})

test.each([
  { overrides: { 'a@1>dep': '1.0.0' } },
  { packageExtensions: { root: { dependencies: { renamed: 'npm:a@^1.0.0' } } } },
  { patchedDependencies: { 'a@1.0.0': 'patches/a.patch' } },
])('a version bump of a workspace project that %j mentions is a structure change', (opts) => {
  const root = { name: 'root', version: '1.0.0' }
  expect(findWorkspaceStructureIssue(context(bumpA(root), opts))).toBe(STRUCTURE_CHANGED)
})

test('every version bump in a workspace is a structure change under deep workspace linking', () => {
  const root = { name: 'root', version: '1.0.0' }
  expect(findWorkspaceStructureIssue(context(bumpA(root), { linkWorkspacePackages: 'deep' })))
    .toBe(STRUCTURE_CHANGED)
})

test('every version bump in a workspace is a structure change when a pnpmfile is loaded', () => {
  const root = { name: 'root', version: '1.0.0' }
  expect(findWorkspaceStructureIssue(context(bumpA(root), {}, { pnpmfiles: ['.pnpmfile.cjs'] })))
    .toBe(STRUCTURE_CHANGED)
})

test('a version bump of a lone project is not a structure change when a pnpmfile is loaded', () => {
  const allProjects: ProjectsList = [
    { rootDir: workspaceDir as ProjectRootDir, manifest: { name: 'root', version: '1.0.1' } },
  ]
  expect(findWorkspaceStructureIssue(context(allProjects, {}, { pnpmfiles: ['.pnpmfile.cjs'] })))
    .toBeUndefined()
})

test('a rename of a lone project is a structure change', () => {
  const allProjects: ProjectsList = [
    { rootDir: workspaceDir as ProjectRootDir, manifest: { name: 'renamed', version: '1.0.0' } },
  ]
  expect(findWorkspaceStructureIssue(context(allProjects))).toBe(STRUCTURE_CHANGED)
})

const STRUCTURE_CHANGED = 'The workspace structure has changed since last install'

/** The root, as given, beside `a`, recorded at `1.0.0` and now at `1.0.1`. */
function bumpA (root: ProjectsList[number]['manifest']): ProjectsList {
  return [
    { rootDir: workspaceDir as ProjectRootDir, manifest: root },
    { rootDir: pkgDir as ProjectRootDir, manifest: { name: 'a', version: '1.0.1' } },
  ]
}

function context (
  allProjects: ProjectsList,
  opts: Partial<WorkspaceDepsStatusContext['opts']> = {},
  state: Partial<WorkspaceDepsStatusContext['workspaceState']> & { catalogs?: Catalogs } = {}
): WorkspaceDepsStatusContext {
  const recorded = {
    [workspaceDir]: { name: 'root', version: '1.0.0' },
    [pkgDir]: { name: 'a', version: '1.0.0' },
  }
  const projects = Object.fromEntries(allProjects.map(({ rootDir }) => [rootDir, recorded[rootDir]]))
  return {
    allProjects,
    opts,
    workspaceDir,
    workspaceState: { projects, pnpmfiles: state.pnpmfiles ?? [], settings: { catalogs: state.catalogs } },
  } as unknown as WorkspaceDepsStatusContext
}
