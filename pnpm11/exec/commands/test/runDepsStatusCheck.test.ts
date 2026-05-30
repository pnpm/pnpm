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

const baseOpts = {
  dir: process.cwd(),
  excludeLinksFromLockfile: false,
  linkWorkspacePackages: false,
  preferWorkspacePackages: false,
  pnpmfile: [],
  rootProjectManifestDir: process.cwd(),
  verifyDepsBeforeRun: 'install' as const,
}

test('includes --filter args with dependency selector in the install command when filter is set', async () => {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    workspaceState: undefined,
  })

  await runDepsStatusCheck({
    ...baseOpts,
    filter: ['foo', 'bar...'],
  })

  expect(runPnpmCli).toHaveBeenCalledWith(
    ['install', '--filter=foo...', '--filter=bar...'],
    {
      cwd: process.cwd(),
      reporter: undefined,
    }
  )
})

test('preserves exclusion filter args in the install command', async () => {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    workspaceState: undefined,
  })

  await runDepsStatusCheck({
    ...baseOpts,
    filter: ['foo', '!bar'],
  })

  expect(runPnpmCli).toHaveBeenCalledWith(
    ['install', '--filter=foo...', '--filter=!bar'],
    {
      cwd: process.cwd(),
      reporter: undefined,
    }
  )
})

test('does not add --filter args when filter is empty', async () => {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    workspaceState: undefined,
  })

  await runDepsStatusCheck({
    ...baseOpts,
    filter: [],
  })

  expect(runPnpmCli).toHaveBeenCalledWith(
    ['install'],
    {
      cwd: process.cwd(),
      reporter: undefined,
    }
  )
})

test('does not add --filter args when filter is undefined', async () => {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    workspaceState: undefined,
  })

  await runDepsStatusCheck({
    ...baseOpts,
    filter: undefined,
  })

  expect(runPnpmCli).toHaveBeenCalledWith(
    ['install'],
    {
      cwd: process.cwd(),
      reporter: undefined,
    }
  )
})

test('includes --filter-prod args in the install command when filterProd is set', async () => {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    workspaceState: undefined,
  })

  await runDepsStatusCheck({
    ...baseOpts,
    filterProd: ['foo', '!bar'],
  })

  expect(runPnpmCli).toHaveBeenCalledWith(
    ['install', '--filter-prod=foo...', '--filter-prod=!bar'],
    {
      cwd: process.cwd(),
      reporter: undefined,
    }
  )
})

test('includes both --filter and --filter-prod args when both are set', async () => {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    workspaceState: undefined,
  })

  await runDepsStatusCheck({
    ...baseOpts,
    filter: ['foo'],
    filterProd: ['bar'],
  })

  expect(runPnpmCli).toHaveBeenCalledWith(
    ['install', '--filter=foo...', '--filter-prod=bar...'],
    {
      cwd: process.cwd(),
      reporter: undefined,
    }
  )
})

