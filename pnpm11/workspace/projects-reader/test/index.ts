import path from 'node:path'

import { afterEach, beforeEach, expect, jest, test } from '@jest/globals'
import { logger } from '@pnpm/logger'
import {
  findWorkspaceProjects,
  findWorkspaceProjectsNoCheck,
  findWorkspaceProjectsNoCheckSync,
  findWorkspaceProjectsSync,
} from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest, readWorkspaceManifestSync } from '@pnpm/workspace.workspace-manifest-reader'

beforeEach(() => {
  jest.spyOn(logger, 'warn')
})

afterEach(() => {
  jest.mocked(logger.warn).mockRestore()
})

test('findWorkspaceProjectsNoCheck() skips engine checks', async () => {
  const fixturePath = path.join(import.meta.dirname, '__fixtures__/bad-engine')

  const workspaceManifest = await readWorkspaceManifest(fixturePath)
  if (workspaceManifest?.packages == null) {
    throw new Error(`Unexpected test setup failure. No pnpm-workspace.yaml packages were defined at ${fixturePath}`)
  }

  const pkgs = await findWorkspaceProjectsNoCheck(fixturePath, {
    patterns: workspaceManifest.packages,
  })
  expect(pkgs).toHaveLength(1)
  expect(pkgs[0].manifest.name).toBe('pkg')
})

test('findWorkspaceProjects() outputs warnings for non-root workspace project', async () => {
  const fixturePath = path.join(import.meta.dirname, '__fixtures__/warning-for-non-root-project')

  const workspaceManifest = await readWorkspaceManifest(fixturePath)
  if (workspaceManifest?.packages == null) {
    throw new Error(`Unexpected test setup failure. No pnpm-workspace.yaml packages were defined at ${fixturePath}`)
  }

  const pkgs = await findWorkspaceProjects(fixturePath, {
    patterns: workspaceManifest.packages,
    sharedWorkspaceLockfile: true,
  })
  expect(pkgs).toHaveLength(3)
  const barPath = path.join(fixturePath, 'packages/bar')
  expect(
    jest.mocked(logger.warn).mock.calls
      .sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)))
  ).toStrictEqual([
    [{ prefix: barPath, message: `The field "resolutions" was found in ${barPath}/package.json. This will not take effect. Configure dependency overrides in pnpm-workspace.yaml using the "overrides" field instead.` }],
  ])
})

test('findWorkspaceProjectsNoCheckSync() works synchronously', () => {
  const fixturePath = path.join(import.meta.dirname, '__fixtures__/bad-engine')
  const workspaceManifest = readWorkspaceManifestSync(fixturePath)
  const pkgs = findWorkspaceProjectsNoCheckSync(fixturePath, {
    patterns: workspaceManifest?.packages,
  })
  expect(pkgs).toHaveLength(1)
  expect(pkgs[0].manifest.name).toBe('pkg')
})

test('findWorkspaceProjectsSync() works synchronously', () => {
  const fixturePath = path.join(import.meta.dirname, '__fixtures__/warning-for-non-root-project')
  const workspaceManifest = readWorkspaceManifestSync(fixturePath)
  const pkgs = findWorkspaceProjectsSync(fixturePath, {
    patterns: workspaceManifest?.packages,
    sharedWorkspaceLockfile: true,
  })
  expect(pkgs).toHaveLength(3)
})

