import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import type { checkDepsStatus as checkDepsStatusFn } from '@pnpm/deps.status'
import type { runPnpmCli as runPnpmCliFn } from '@pnpm/exec.pnpm-cli-runner'
import type { ProjectManifest } from '@pnpm/types'

const checkDepsStatus = jest.fn<typeof checkDepsStatusFn>()
const runPnpmCli = jest.fn<typeof runPnpmCliFn>()

jest.unstable_mockModule('@pnpm/deps.status', () => ({
  checkDepsStatus,
}))

jest.unstable_mockModule('@pnpm/exec.pnpm-cli-runner', () => ({
  runPnpmCli,
}))

const actualLogger = await import('@pnpm/logger')
jest.unstable_mockModule('@pnpm/logger', () => ({
  ...actualLogger,
  globalWarn: jest.fn(),
}))

const confirm = jest.fn<() => Promise<boolean>>()
jest.unstable_mockModule('@inquirer/prompts', () => ({
  confirm,
}))

const { runDepsStatusCheck } = await import('../src/runDepsStatusCheck.js')

beforeEach(() => {
  checkDepsStatus.mockReset()
  runPnpmCli.mockReset()
  confirm.mockReset()
})

test('does not install when dependency status is unavailable without a project manifest', async () => {
  checkDepsStatus.mockResolvedValue({
    upToDate: undefined,
    issue: 'No package manifest found. Skipping check.',
    workspaceState: undefined,
  })

  await runDepsStatusCheck({
    dir: process.cwd(),
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: false,
    preferWorkspacePackages: false,
    pnpmfile: [],
    rootProjectManifestDir: process.cwd(),
    verifyDepsBeforeRun: 'install',
  })

  expect(runPnpmCli).not.toHaveBeenCalled()
})

test('installs when dependency status is unavailable for an unexpected reason', async () => {
  checkDepsStatus.mockResolvedValue({
    upToDate: undefined,
    issue: 'Cannot verify dependency status',
    workspaceState: undefined,
  })

  await runDepsStatusCheck({
    dir: process.cwd(),
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: false,
    pnpmfile: [],
    preferWorkspacePackages: false,
    rootProjectManifest: {
      name: 'root',
    },
    rootProjectManifestDir: process.cwd(),
    verifyDepsBeforeRun: 'install',
  })

  expect(runPnpmCli).toHaveBeenCalledWith(['install'], {
    cwd: process.cwd(),
    reporter: undefined,
  })
})

test('ignore-workspace does not read an enclosing workspace manifest', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-ignore-workspace-'))
  const project = path.join(root, 'project')
  fs.mkdirSync(project)
  fs.writeFileSync(path.join(root, 'pnpm-workspace.yml'), 'packages:\n  - .\n')
  checkDepsStatus.mockResolvedValue({
    upToDate: undefined,
    issue: 'Cannot check whether dependencies are outdated',
    workspaceState: undefined,
  })

  await runDepsStatusCheck({
    dir: project,
    excludeLinksFromLockfile: false,
    ignoreWorkspace: true,
    linkWorkspacePackages: false,
    pnpmfile: [],
    preferWorkspacePackages: false,
    rootProjectManifest: {
      name: 'left-out',
    },
    rootProjectManifestDir: project,
    verifyDepsBeforeRun: 'install',
  })

  expect(runPnpmCli).toHaveBeenCalledWith(['install'], {
    cwd: project,
    reporter: undefined,
  })
})

test('installs only the selected projects when a filter is set', async () => {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    issue: 'The workspace structure has changed since last install',
    workspaceState: undefined,
  })

  await runDepsStatusCheck({
    dir: process.cwd(),
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: false,
    pnpmfile: [],
    preferWorkspacePackages: false,
    rootProjectManifest: {
      name: 'root',
    },
    rootProjectManifestDir: process.cwd(),
    verifyDepsBeforeRun: 'install',
    filter: ['project-b'],
    filterProd: ['project-c'],
  })

  expect(runPnpmCli).toHaveBeenCalledWith(['install', '--filter=project-b...', '--filter-prod=project-c...'], {
    cwd: process.cwd(),
    reporter: undefined,
  })
})

test('install refuses when the lockfile would lose settings of the ignored "pnpm" field', async () => {
  mockOutdatedStatus()

  await expect(runDepsStatusCheck(optsWithPnpmField('install'))).rejects.toMatchObject({
    code: 'ERR_PNPM_VERIFY_DEPS_BEFORE_RUN',
    message: 'Your node_modules are out of sync with your lockfile, and installing would drop "pnpm.overrides" from the lockfile, because the "pnpm" field in package.json is no longer read by pnpm',
  })

  expect(runPnpmCli).not.toHaveBeenCalled()
})

test('a confirmed prompt refuses when the lockfile would lose settings of the ignored "pnpm" field', async () => {
  mockOutdatedStatus()
  confirm.mockResolvedValue(true)

  await withTTY(async () => {
    await expect(runDepsStatusCheck(optsWithPnpmField('prompt'))).rejects.toMatchObject({
      code: 'ERR_PNPM_VERIFY_DEPS_BEFORE_RUN',
    })
  })

  expect(runPnpmCli).not.toHaveBeenCalled()
})

test('a prompt that cannot ask refuses when the lockfile would lose settings of the ignored "pnpm" field', async () => {
  mockOutdatedStatus()
  const isTTY = process.stdin.isTTY
  process.stdin.isTTY = false
  try {
    await expect(runDepsStatusCheck(optsWithPnpmField('prompt'))).rejects.toMatchObject({
      message: expect.stringContaining('installing would drop "pnpm.overrides" from the lockfile'),
    })
  } finally {
    process.stdin.isTTY = isTTY
  }

  expect(confirm).not.toHaveBeenCalled()
})

test('a declined prompt lets the command run despite settings in the ignored "pnpm" field', async () => {
  mockOutdatedStatus()
  confirm.mockResolvedValue(false)

  await withTTY(() => runDepsStatusCheck(optsWithPnpmField('prompt')))

  expect(runPnpmCli).not.toHaveBeenCalled()
})

test('installs when the ignored "pnpm" field holds no setting the lockfile records', async () => {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    issue: 'The lockfile settings are outdated',
    workspaceState: undefined,
  })

  await runDepsStatusCheck({
    dir: process.cwd(),
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: false,
    preferWorkspacePackages: false,
    pnpmfile: [],
    rootProjectManifest: withPnpmField({ onlyBuiltDependencies: [] }),
    rootProjectManifestDir: process.cwd(),
    verifyDepsBeforeRun: 'install',
  })

  expect(runPnpmCli).toHaveBeenCalledTimes(1)
})

function withPnpmField (pnpm: Record<string, unknown>): ProjectManifest {
  const manifest = { name: 'root', pnpm }
  return manifest
}

function mockOutdatedStatus (): void {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    issue: 'The lockfile settings are outdated',
    workspaceState: undefined,
  })
}

function optsWithPnpmField (verifyDepsBeforeRun: 'install' | 'prompt'): Parameters<typeof runDepsStatusCheck>[0] {
  return {
    dir: process.cwd(),
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: false,
    preferWorkspacePackages: false,
    pnpmfile: [],
    rootProjectManifest: withPnpmField({ overrides: { foo: '1.0.0' }, onlyBuiltDependencies: [] }),
    rootProjectManifestDir: process.cwd(),
    verifyDepsBeforeRun,
  }
}

async function withTTY (fn: () => Promise<void>): Promise<void> {
  const isTTY = process.stdin.isTTY
  process.stdin.isTTY = true
  try {
    await fn()
  } finally {
    process.stdin.isTTY = isTTY
  }
}
