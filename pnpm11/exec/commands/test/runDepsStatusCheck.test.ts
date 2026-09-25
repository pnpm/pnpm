import { beforeEach, expect, jest, test } from '@jest/globals'
import type { checkDepsStatus as checkDepsStatusFn } from '@pnpm/deps.status'
import type { runPnpmCli as runPnpmCliFn } from '@pnpm/exec.pnpm-cli-runner'

const checkDepsStatus = jest.fn<typeof checkDepsStatusFn>()
const installedModulesMatchLockfile = jest.fn<() => Promise<boolean>>()
const runPnpmCli = jest.fn<typeof runPnpmCliFn>()

jest.unstable_mockModule('@pnpm/deps.status', () => ({
  CANNOT_CHECK_DEPS_ISSUE: 'Cannot check whether dependencies are outdated',
  checkDepsStatus,
  installedModulesMatchLockfile,
}))

jest.unstable_mockModule('@pnpm/exec.pnpm-cli-runner', () => ({
  runPnpmCli,
}))

jest.unstable_mockModule('@pnpm/logger', () => ({
  globalWarn: jest.fn(),
}))

jest.unstable_mockModule('@inquirer/prompts', () => ({
  confirm: jest.fn(),
}))

const { runDepsStatusCheck } = await import('../src/runDepsStatusCheck.js')

beforeEach(() => {
  checkDepsStatus.mockReset()
  installedModulesMatchLockfile.mockReset()
  installedModulesMatchLockfile.mockResolvedValue(false)
  runPnpmCli.mockReset()
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

test('does not install when the state file cannot be read but node_modules matches the lockfile', async () => {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    issue: 'Cannot check whether dependencies are outdated',
    workspaceState: undefined,
  })
  installedModulesMatchLockfile.mockResolvedValue(true)

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

  expect(installedModulesMatchLockfile).toHaveBeenCalled()
  expect(runPnpmCli).not.toHaveBeenCalled()
})
