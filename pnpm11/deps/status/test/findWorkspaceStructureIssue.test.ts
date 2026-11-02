import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { ProjectRootDir } from '@pnpm/types'
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

test('a version bump in a workspace of several projects is a structure change', () => {
  const allProjects: ProjectsList = [
    { rootDir: workspaceDir as ProjectRootDir, manifest: { name: 'root', version: '1.0.0' } },
    { rootDir: pkgDir as ProjectRootDir, manifest: { name: 'a', version: '1.0.1' } },
  ]
  expect(findWorkspaceStructureIssue(context(allProjects))).toBe('The workspace structure has changed since last install')
})

test('a rename of a lone project is a structure change', () => {
  const allProjects: ProjectsList = [
    { rootDir: workspaceDir as ProjectRootDir, manifest: { name: 'renamed', version: '1.0.0' } },
  ]
  expect(findWorkspaceStructureIssue(context(allProjects))).toBe('The workspace structure has changed since last install')
})

function context (allProjects: ProjectsList): WorkspaceDepsStatusContext {
  const recorded = {
    [workspaceDir]: { name: 'root', version: '1.0.0' },
    [pkgDir]: { name: 'a', version: '1.0.0' },
  }
  const projects = Object.fromEntries(allProjects.map(({ rootDir }) => [rootDir, recorded[rootDir]]))
  return {
    allProjects,
    opts: {},
    workspaceDir,
    workspaceState: { projects, settings: {} },
  } as unknown as WorkspaceDepsStatusContext
}
