import { beforeEach, expect, jest, test } from '@jest/globals'
import type { checkDepsStatus as checkDepsStatusFn } from '@pnpm/deps.status'
import type { runPnpmCli as runPnpmCliFn } from '@pnpm/exec.pnpm-cli-runner'

const checkDepsStatus = jest.fn<typeof checkDepsStatusFn>()
const runPnpmCli = jest.fn<typeof runPnpmCliFn>()

jest.unstable_mockModule('@pnpm/deps.status', () => ({
  checkDepsStatus,
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
