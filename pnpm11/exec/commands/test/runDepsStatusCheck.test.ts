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

test.each(['install', 'prompt'] as const)('%s refuses to install when the lockfile would lose settings of the ignored "pnpm" field', async (verifyDepsBeforeRun) => {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    issue: 'The lockfile settings are outdated',
    workspaceState: undefined,
  })

  await expect(runDepsStatusCheck({
    dir: process.cwd(),
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: false,
    preferWorkspacePackages: false,
    pnpmfile: [],
    rootProjectManifest: withPnpmField({ overrides: { foo: '1.0.0' }, onlyBuiltDependencies: [] }),
    rootProjectManifestDir: process.cwd(),
    verifyDepsBeforeRun,
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_VERIFY_DEPS_BEFORE_RUN',
    message: 'Your node_modules are out of sync with your lockfile, and installing would drop "pnpm.overrides" from the lockfile, because the "pnpm" field in package.json is no longer read by pnpm',
  })

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
