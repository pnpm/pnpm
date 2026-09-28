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
type RunDepsStatusCheckOptions = Parameters<typeof runDepsStatusCheck>[0]

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
      dependencies: { a: '1.0.0' },
    },
    rootProjectManifestDir: process.cwd(),
    verifyDepsBeforeRun: 'install',
  })

  expect(runPnpmCli).toHaveBeenCalledWith(['install'], {
    cwd: process.cwd(),
    reporter: undefined,
  })
})

test('does not install a never-installed project that has nothing to install', async () => {
  const project = projectDir()
  await runWithoutWorkspaceState(project, { rootProjectManifest: { name: 'scripts-only', scripts: { hi: 'echo hi' } } })

  expect(runPnpmCli).not.toHaveBeenCalled()
})

test.each([
  ['a dependency', { devDependencies: { a: '1.0.0' } }],
  ['an install lifecycle script', { scripts: { prepare: 'echo prepare' } }],
  ['a pnpm:devPreinstall script', { scripts: { 'pnpm:devPreinstall': 'echo dev' } }],
] satisfies Array<[string, Partial<ProjectManifest>]>)('installs a never-installed project that declares %s', async (_, fields) => {
  const project = projectDir()
  await runWithoutWorkspaceState(project, { rootProjectManifest: { name: 'project', ...fields } })

  expect(runPnpmCli).toHaveBeenCalledWith(['install'], { cwd: project, reporter: undefined })
})

test('does not install a never-installed project whose install scripts are ignored', async () => {
  const project = projectDir()
  await runWithoutWorkspaceState(project, {
    ignoreScripts: true,
    rootProjectManifest: { name: 'prepare-only', scripts: { prepare: 'echo prepare' } },
  })

  expect(runPnpmCli).not.toHaveBeenCalled()
})

test('a pnpm:devPreinstall script of a workspace member does not start an install', async () => {
  const workspace = projectDir()
  await runWithoutWorkspaceState(workspace, {
    rootProjectManifest: { name: 'root' },
    allProjects: [
      { rootDir: workspace, manifest: { name: 'root' } },
      { rootDir: path.join(workspace, 'pkgs/a'), manifest: { name: 'a', scripts: { 'pnpm:devPreinstall': 'echo dev' } } },
    ] as RunDepsStatusCheckOptions['allProjects'],
    workspaceDir: workspace,
  })

  expect(runPnpmCli).not.toHaveBeenCalled()
})

test('installs a never-installed project that has a binding.gyp', async () => {
  const project = projectDir()
  fs.writeFileSync(path.join(project, 'binding.gyp'), '{}')
  await runWithoutWorkspaceState(project, { rootProjectManifest: { name: 'native' } })

  expect(runPnpmCli).toHaveBeenCalledWith(['install'], { cwd: project, reporter: undefined })
})

test('installs a never-installed workspace when any project has a dependency', async () => {
  const workspace = projectDir()
  await runWithoutWorkspaceState(workspace, {
    rootProjectManifest: { name: 'root' },
    allProjects: [
      { rootDir: workspace, manifest: { name: 'root' } },
      { rootDir: path.join(workspace, 'pkgs/a'), manifest: { name: 'a', dependencies: { b: '1.0.0' } } },
    ] as RunDepsStatusCheckOptions['allProjects'],
    workspaceDir: workspace,
  })

  expect(runPnpmCli).toHaveBeenCalledWith(['install'], { cwd: workspace, reporter: undefined })
})

test('a non-recursive command installs a never-installed workspace whose member has a dependency', async () => {
  const workspace = projectDir()
  fs.writeFileSync(path.join(workspace, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.mkdirSync(path.join(workspace, 'pkgs/a'), { recursive: true })
  fs.writeFileSync(path.join(workspace, 'pkgs/a/package.json'), JSON.stringify({ name: 'a', dependencies: { b: '1.0.0' } }))
  await runWithoutWorkspaceState(workspace, {
    rootProjectManifest: { name: 'root' },
    workspaceDir: workspace,
    workspacePackagePatterns: ['pkgs/*'],
  })

  expect(runPnpmCli).toHaveBeenCalledWith(['install'], { cwd: workspace, reporter: undefined })
})

test('a non-recursive command skips the install of a never-installed workspace with nothing to install', async () => {
  const workspace = projectDir()
  fs.writeFileSync(path.join(workspace, 'package.json'), JSON.stringify({ name: 'root' }))
  fs.mkdirSync(path.join(workspace, 'pkgs/a'), { recursive: true })
  fs.writeFileSync(path.join(workspace, 'pkgs/a/package.json'), JSON.stringify({ name: 'a' }))
  await runWithoutWorkspaceState(workspace, {
    rootProjectManifest: { name: 'root' },
    workspaceDir: workspace,
    workspacePackagePatterns: ['pkgs/*'],
  })

  expect(runPnpmCli).not.toHaveBeenCalled()
})

test('with separate lockfiles, a sibling\'s dependencies do not start an install', async () => {
  const workspace = projectDir()
  const project = path.join(workspace, 'pkgs/a')
  fs.mkdirSync(project, { recursive: true })
  fs.writeFileSync(path.join(project, 'package.json'), JSON.stringify({ name: 'a' }))
  fs.mkdirSync(path.join(workspace, 'pkgs/b'), { recursive: true })
  fs.writeFileSync(path.join(workspace, 'pkgs/b/package.json'), JSON.stringify({ name: 'b', dependencies: { c: '1.0.0' } }))
  await runWithoutWorkspaceState(project, {
    rootProjectManifest: { name: 'root', dependencies: { d: '1.0.0' } },
    rootProjectManifestDir: workspace,
    sharedWorkspaceLockfile: false,
    workspaceDir: workspace,
    workspacePackagePatterns: ['pkgs/*'],
  })

  expect(runPnpmCli).not.toHaveBeenCalled()
})

test('a required peer is installed when auto-install-peers is on', async () => {
  const project = projectDir()
  await runWithoutWorkspaceState(project, {
    autoInstallPeers: true,
    rootProjectManifest: { name: 'peers', peerDependencies: { a: '1.0.0' } },
  })

  expect(runPnpmCli).toHaveBeenCalledWith(['install'], { cwd: project, reporter: undefined })
})

test('an optional peer is not installed', async () => {
  const project = projectDir()
  await runWithoutWorkspaceState(project, {
    autoInstallPeers: true,
    rootProjectManifest: {
      name: 'peers',
      peerDependencies: { a: '1.0.0' },
      peerDependenciesMeta: { a: { optional: true } },
    },
  })

  expect(runPnpmCli).not.toHaveBeenCalled()
})

test('a required peer is not installed when auto-install-peers is off', async () => {
  const project = projectDir()
  await runWithoutWorkspaceState(project, {
    autoInstallPeers: false,
    rootProjectManifest: { name: 'peers', peerDependencies: { a: '1.0.0' } },
  })

  expect(runPnpmCli).not.toHaveBeenCalled()
})

function projectDir (): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-verify-deps-'))
}

async function runWithoutWorkspaceState (dir: string, opts: Partial<RunDepsStatusCheckOptions>): Promise<void> {
  checkDepsStatus.mockResolvedValue({
    upToDate: false,
    issue: 'Cannot check whether dependencies are outdated',
    workspaceState: undefined,
  })
  await runDepsStatusCheck({
    dir,
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: false,
    pnpmfile: [],
    preferWorkspacePackages: false,
    rootProjectManifestDir: dir,
    verifyDepsBeforeRun: 'install',
    ...opts,
  })
}

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
      dependencies: { a: '1.0.0' },
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
  const manifest = { name: 'root', dependencies: { foo: '1.0.0' }, pnpm }
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
