import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { ProjectRootDir } from '@pnpm/types'
import { type ProjectsList, type Validation, workspaceBinsDigest } from '@pnpm/workspace.state'

import type { WorkspaceDepsStatusContext } from '../src/checkWorkspaceDepsStatus.js'
import { findWorkspaceBinsIssue } from '../src/findWorkspaceStructureIssue.js'

const workspaceDir = path.resolve('workspace')
const allProjects: ProjectsList = [
  { rootDir: workspaceDir as ProjectRootDir, manifest: { name: 'root', dependencies: { a: 'workspace:*' } } },
  { rootDir: path.join(workspaceDir, 'packages/a') as ProjectRootDir, manifest: { name: 'a', bin: { 'a-one': 'one.js' } } },
]
const workspaceBins = workspaceBinsDigest(workspaceDir, allProjects)

test.each<[string, Validation | undefined, string | undefined]>([
  ['a state without a validation record', undefined, undefined],
  ['a validation record without workspace bins', validation({}), 'workspace package bins changed since the last install'],
  ['a validation record with other workspace bins', validation({ '.': 'other' }), 'workspace package bins changed since the last install'],
  ['a validation record with the current workspace bins', validation({ '.': workspaceBins! }), undefined],
])('%s', (_, recorded, expected) => {
  const ctx = {
    allProjects,
    opts: { lockfileDir: workspaceDir },
    workspaceDir,
    workspaceState: { validation: recorded },
  } as unknown as WorkspaceDepsStatusContext
  expect(findWorkspaceBinsIssue(ctx, () => true, workspaceBins)).toBe(expected)
})

function validation (workspaceBins: Record<string, string>): Validation {
  return { recordedAtNs: 0, files: {}, manifests: {}, workspaceBins }
}
