import { rmSync, statSync, writeFileSync } from 'node:fs'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterEach, describe, expect, it } from '@jest/globals'
import { checkDepsStatus, type CheckDepsStatusOptions } from '@pnpm/deps.status'
import { type LockfileObject, readCurrentLockfile, writeCurrentLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { DepPath, Project, ProjectId, ProjectManifest, ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
import { loadWorkspaceState, updateWorkspaceState } from '@pnpm/workspace.state'

const SETTINGS = {
  autoInstallPeers: true,
  peersSuffixMaxLength: 1000,
  excludeLinksFromLockfile: false,
  linkWorkspacePackages: true,
  preferWorkspacePackages: true,
}

function lockfileWith (bDependencies: Record<string, string>): LockfileObject {
  return {
    lockfileVersion: '9.0',
    settings: { autoInstallPeers: true, excludeLinksFromLockfile: false },
    importers: {
      ['.' as ProjectId]: { specifiers: {} },
      ['packages/a' as ProjectId]: { specifiers: { foo: '1.0.0' }, dependencies: { foo: '1.0.0' } },
      ['packages/b' as ProjectId]: { specifiers: bDependencies, dependencies: bDependencies },
    },
    packages: {
      ['bar@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-bar' } },
      ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-foo' } },
    },
  }
}

interface RecordedWorkspace {
  workspaceDir: string
  allProjects: Project[]
  rootManifest: ProjectManifest
  projectDir: (name: string) => string
  manifestOf: (name: string) => ProjectManifest
}

const workspaces: string[] = []

afterEach(async () => {
  await Promise.all(workspaces.splice(0).map(async (dir) => fs.rm(dir, { recursive: true, force: true })))
})

/**
 * A workspace whose projects `a` and `b` are installed from `lockfile`, as
 * an install of every project leaves it.
 */
async function installedWorkspace (lockfile: LockfileObject): Promise<RecordedWorkspace> {
  const workspaceDir = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-recorded-validation-')))
  workspaces.push(workspaceDir)
  const rootManifest: ProjectManifest = { name: 'root', version: '1.0.0' }
  const manifests: Record<string, ProjectManifest> = {
    a: { name: 'a', version: '1.0.0', dependencies: { foo: '1.0.0' } },
    b: { name: 'b', version: '1.0.0', dependencies: lockfile.importers['packages/b' as ProjectId].dependencies },
  }
  const projectDir = (name: string): string => name === 'root' ? workspaceDir : path.join(workspaceDir, 'packages', name)
  await fs.writeFile(path.join(workspaceDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
  await fs.writeFile(path.join(workspaceDir, 'package.json'), JSON.stringify(rootManifest))
  await Promise.all(Object.entries(manifests).map(async ([name, manifest]) => {
    await fs.mkdir(path.join(projectDir(name), 'node_modules'), { recursive: true })
    await fs.writeFile(path.join(projectDir(name), 'package.json'), JSON.stringify(manifest))
  }))
  await writeWantedLockfile(workspaceDir, lockfile)
  await writeCurrentLockfile(path.join(workspaceDir, 'node_modules/.pnpm'), lockfile)
  const allProjects = [['root', rootManifest] as const, ...Object.entries(manifests)].map(([name, manifest]) => ({
    rootDir: projectDir(name) as ProjectRootDir,
    rootDirRealPath: projectDir(name) as ProjectRootDirRealPath,
    manifest,
    writeProjectManifest: async () => {},
  }))
  const workspace = { workspaceDir, allProjects, rootManifest, projectDir, manifestOf: (name: string) => manifests[name] }
  await recordInstall(workspace)
  return workspace
}

const CONFIG_DEPENDENCIES = { 'pnpm-plugin-x': '1.0.0+sha512-x' }

/** Record the state an install leaves, vouching for `vouched` projects only when set. */
async function recordInstall (workspace: RecordedWorkspace, vouched?: string[]): Promise<void> {
  if (vouched != null && loadWorkspaceState(workspace.workspaceDir) == null) {
    const virtualStoreDir = path.join(workspace.workspaceDir, 'node_modules/.pnpm')
    const current = await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
    if (current?.packages != null) {
      if (!vouched.includes('a')) delete current.packages['foo@1.0.0' as DepPath]
      if (!vouched.includes('b')) delete current.packages['bar@1.0.0' as DepPath]
      await writeCurrentLockfile(virtualStoreDir, current)
    }
  }
  await updateWorkspaceState({
    allProjects: workspace.allProjects,
    workspaceDir: workspace.workspaceDir,
    pnpmfiles: [],
    settings: SETTINGS,
    configDependencies: CONFIG_DEPENDENCIES,
    filteredInstall: vouched != null,
    validation: {
      lockfileDir: workspace.workspaceDir,
      wantedLockfileName: 'pnpm-lock.yaml',
      sharedInputs: [],
      vouchedProjects: vouched?.map(workspace.projectDir),
    },
  })
}

function check (
  workspace: RecordedWorkspace,
  selected?: string[],
  extraOpts?: Partial<CheckDepsStatusOptions>
): ReturnType<typeof checkDepsStatus> {
  const selectedProjectsGraph = selected == null
    ? undefined
    : Object.fromEntries(selected.map((name) => {
      const project = workspace.allProjects.find(({ manifest }) => manifest.name === name)!
      return [project.rootDir, { dependencies: [], package: project }]
    }))
  const opts: CheckDepsStatusOptions = {
    ...SETTINGS,
    allProjects: workspace.allProjects,
    workspaceDir: workspace.workspaceDir,
    rootProjectManifestDir: workspace.workspaceDir,
    rootProjectManifest: workspace.rootManifest,
    sharedWorkspaceLockfile: true,
    pnpmfile: [],
    configDependencies: CONFIG_DEPENDENCIES,
    selectedProjectsGraph,
    ...extraOpts,
  } as CheckDepsStatusOptions
  return checkDepsStatus(opts)
}

describe('checkDepsStatus with recorded validations', () => {
  it('is up to date when every project is proven installed', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    expect(await check(workspace)).toMatchObject({ upToDate: true })
  })

  it('stays up to date when the lockfile is rewritten with the same content', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    const lockfilePath = path.join(workspace.workspaceDir, 'pnpm-lock.yaml')
    await fs.writeFile(lockfilePath, await fs.readFile(lockfilePath))
    expect(await check(workspace)).toMatchObject({ upToDate: true })
  })

  // `cp -p`, `rsync -a`, an archive extraction, or a Docker `COPY` of an older
  // file leave the mtime where it was.
  it('is outdated after a manifest edit that kept its mtime', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    const manifestPath = path.join(workspace.projectDir('b'), 'package.json')
    const { mtime } = await fs.stat(manifestPath)
    workspace.manifestOf('b').dependencies = { bar: '1.0.0', foo: '1.0.0' }
    await fs.writeFile(manifestPath, JSON.stringify(workspace.manifestOf('b')))
    await fs.utimes(manifestPath, mtime, mtime)
    expect(await check(workspace)).toMatchObject({ upToDate: false })
  })

  it('holds a selected project a filtered install never installed to an install', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    await fs.rm(path.join(workspace.workspaceDir, 'node_modules/.pnpm-workspace-state-v1.json'))
    await recordInstall(workspace, ['a'])
    expect(await check(workspace, ['a'])).toMatchObject({ upToDate: true })
    expect(await check(workspace, ['b'])).toMatchObject({
      upToDate: false,
      issue: 'Workspace package b has dependencies but was not installed',
    })
  })

  it('holds a project a filtered install never installed when the wanted lockfile is missing', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    await fs.rm(path.join(workspace.workspaceDir, 'node_modules/.pnpm-workspace-state-v1.json'))
    await recordInstall(workspace, ['a'])
    await fs.rm(path.join(workspace.workspaceDir, 'pnpm-lock.yaml'))
    expect(await check(workspace, ['a'])).toMatchObject({ upToDate: true })
    expect(await check(workspace, ['b'])).toMatchObject({
      upToDate: false,
      issue: 'Workspace package b has dependencies but was not installed',
    })
    expect(await check(workspace)).toMatchObject({
      upToDate: false,
      issue: 'Workspace package b has dependencies but was not installed',
    })
  })

  it('holds a project with an empty current-lockfile importer when the wanted lockfile is missing', async () => {
    const workspace = await installedWorkspace(lockfileWith({ foo: '1.0.0' }))
    await fs.rm(path.join(workspace.workspaceDir, 'node_modules/.pnpm-workspace-state-v1.json'))
    const virtualStoreDir = path.join(workspace.workspaceDir, 'node_modules/.pnpm')
    const current = (await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false }))!
    current.importers['packages/b' as ProjectId] = { specifiers: {} }
    await writeCurrentLockfile(virtualStoreDir, current)
    await recordInstall(workspace, ['a'])
    await fs.rm(path.join(workspace.workspaceDir, 'pnpm-lock.yaml'))
    expect(await check(workspace, ['a'])).toMatchObject({ upToDate: true })
    expect(await check(workspace, ['b'])).toMatchObject({
      upToDate: false,
      issue: 'Workspace package b has dependencies but was not installed',
    })
    expect(await check(workspace)).toMatchObject({
      upToDate: false,
      issue: 'Workspace package b has dependencies but was not installed',
    })
  })

  // https://github.com/pnpm/pnpm/issues/17011
  it('holds a selected project installed from an earlier lockfile to an install after a filtered install', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    workspace.manifestOf('b').dependencies = { bar: '1.0.0', foo: '1.0.0' }
    await writeWantedLockfile(workspace.workspaceDir, lockfileWith({ bar: '1.0.0', foo: '1.0.0' }))
    await recordInstall(workspace, ['a'])
    expect(await check(workspace, ['a'])).toMatchObject({ upToDate: true })
    expect(await check(workspace, ['b'])).toMatchObject({
      upToDate: false,
      issue: 'The installed dependencies of b are not up to date with the lockfile',
    })
  })

  it('holds a project a filtered install left out to its edits', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    workspace.manifestOf('b').dependencies = { foo: '1.0.0' }
    await recordInstall(workspace, ['a'])
    expect(await check(workspace, ['b'])).toMatchObject({ upToDate: false })
  })

  it('keeps the configuration dependencies when it records a passing check', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    workspace.manifestOf('b').description = 'edited'
    expect(await check(workspace)).toMatchObject({ upToDate: true })
    expect(loadWorkspaceState(workspace.workspaceDir)?.configDependencies).toStrictEqual(CONFIG_DEPENDENCIES)
    expect(await check(workspace)).toMatchObject({ upToDate: true })
  })

  it('restores a deleted wanted lockfile from the current one', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    const current = lockfileWith({ bar: '1.0.0' })
    current.packages!['baz@1.0.0' as DepPath] = { resolution: { integrity: 'sha512-baz' } }
    await writeCurrentLockfile(path.join(workspace.workspaceDir, 'node_modules/.pnpm'), current)
    await fs.rm(path.join(workspace.workspaceDir, 'pnpm-lock.yaml'))
    const result = await check(workspace)
    expect(result).toMatchObject({ upToDate: true })
    expect(result.wantedLockfileToRestore).toBeDefined()
  })

  // A lockfile written in the clock tick its validation was recorded in is
  // read again, and recorded again once that record can be trusted.
  it('records the stat of a lockfile it read again and found unchanged', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    const statePath = path.join(workspace.workspaceDir, 'node_modules/.pnpm-workspace-state-v1.json')
    const state = loadWorkspaceState(workspace.workspaceDir)!
    const lockfilePath = path.join(workspace.workspaceDir, 'pnpm-lock.yaml')
    const signature = state.validation!.files![lockfilePath]
    state.validation!.recordedAtNs = Math.max(signature.mtimeNs, signature.ctimeNs ?? 0)
    await fs.writeFile(statePath, JSON.stringify(state))
    waitForFilesystemClockPast(workspace.workspaceDir, state.validation!.recordedAtNs)
    expect(await check(workspace)).toMatchObject({ upToDate: true })
    const refreshed = loadWorkspaceState(workspace.workspaceDir)!
    expect(refreshed.validation!.recordedAtNs).toBeGreaterThan(state.validation!.recordedAtNs)
    expect(refreshed.validation!.manifests).toStrictEqual(state.validation!.manifests)
  })

  // The recorded digest covers pnpm-lock.yaml alone, so a branch lockfile
  // merged into the wanted one leaves the check to the lockfiles.
  it('notices a branch lockfile that changes what the merged lockfile wants', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    const branchDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-branch-lockfile-'))
    workspaces.push(branchDir)
    await writeWantedLockfile(branchDir, lockfileWith({ bar: '1.0.0', foo: '1.0.0' }))
    await fs.copyFile(path.join(branchDir, 'pnpm-lock.yaml'), path.join(workspace.workspaceDir, 'pnpm-lock.feature.yaml'))
    expect(await check(workspace, undefined, { mergeGitBranchLockfiles: true })).toMatchObject({ upToDate: false })
  })

  it('keeps the recorded proofs while no branch lockfile waits to be merged', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    expect(await check(workspace, undefined, { mergeGitBranchLockfiles: true })).toMatchObject({ upToDate: true })
    const manifestPath = path.join(workspace.projectDir('b'), 'package.json')
    const { mtime } = await fs.stat(manifestPath)
    workspace.manifestOf('b').dependencies = { bar: '1.0.0', foo: '1.0.0' }
    await fs.writeFile(manifestPath, JSON.stringify(workspace.manifestOf('b')))
    await fs.utimes(manifestPath, mtime, mtime)
    expect(await check(workspace, undefined, { mergeGitBranchLockfiles: true })).toMatchObject({ upToDate: false })
  })

  // https://github.com/pnpm/pnpm/issues/17127
  it('applies overrides when checking a modified manifest against the lockfile', async () => {
    const overrides = { 'bar@^2.0.0': '1.0.0', 'b>a': 'workspace:*' }
    const lockfile = lockfileWith({ a: 'link:../a', bar: '1.0.0' })
    lockfile.importers['packages/b' as ProjectId].specifiers = { a: 'workspace:*', bar: '1.0.0' }
    lockfile.overrides = overrides
    const workspace = await installedWorkspace(lockfile)
    workspace.manifestOf('b').dependencies = { a: '^2.0.0', bar: '^2.0.0' }
    await fs.writeFile(path.join(workspace.projectDir('b'), 'package.json'), JSON.stringify(workspace.manifestOf('b')))
    await updateWorkspaceState({
      allProjects: workspace.allProjects,
      workspaceDir: workspace.workspaceDir,
      pnpmfiles: [],
      settings: { ...SETTINGS, overrides },
      configDependencies: CONFIG_DEPENDENCIES,
      filteredInstall: false,
      validation: {
        lockfileDir: workspace.workspaceDir,
        wantedLockfileName: 'pnpm-lock.yaml',
        sharedInputs: [],
      },
    })

    workspace.manifestOf('b').description = 'edited'
    await fs.writeFile(path.join(workspace.projectDir('b'), 'package.json'), JSON.stringify(workspace.manifestOf('b')))
    expect(await check(workspace, undefined, { overrides })).toMatchObject({ upToDate: true })

    // Also covers the non-workspace root project path when package.json is edited.
    await fs.rm(path.join(workspace.workspaceDir, 'pnpm-workspace.yaml'))
    workspace.rootManifest.dependencies = { bar: '^2.0.0' }
    lockfile.importers['.' as ProjectId] = { specifiers: { bar: '1.0.0' }, dependencies: { bar: '1.0.0' } }
    await writeWantedLockfile(workspace.workspaceDir, lockfile)
    await writeCurrentLockfile(path.join(workspace.workspaceDir, 'node_modules/.pnpm'), lockfile)
    await updateWorkspaceState({
      allProjects: [workspace.allProjects[0]],
      workspaceDir: workspace.workspaceDir,
      pnpmfiles: [],
      settings: { ...SETTINGS, overrides },
      configDependencies: CONFIG_DEPENDENCIES,
      filteredInstall: false,
      validation: {
        lockfileDir: workspace.workspaceDir,
        wantedLockfileName: 'pnpm-lock.yaml',
        sharedInputs: [],
      },
    })
    workspace.rootManifest.description = 'edited root'
    await fs.writeFile(path.join(workspace.workspaceDir, 'package.json'), JSON.stringify(workspace.rootManifest))
    expect(await checkDepsStatus({
      ...SETTINGS,
      overrides,
      rootProjectManifestDir: workspace.workspaceDir,
      rootProjectManifest: workspace.rootManifest,
      pnpmfile: [],
      configDependencies: CONFIG_DEPENDENCIES,
    })).toMatchObject({ upToDate: true })
  })

  it('preserves ignoredWorkspaceStateSettings and autoDedupe when refreshing the workspace state', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    await updateWorkspaceState({
      allProjects: workspace.allProjects,
      workspaceDir: workspace.workspaceDir,
      pnpmfiles: [],
      settings: { ...SETTINGS, autoDedupe: true, dev: false, optional: true, production: true },
      configDependencies: CONFIG_DEPENDENCIES,
      filteredInstall: false,
      validation: {
        lockfileDir: workspace.workspaceDir,
        wantedLockfileName: 'pnpm-lock.yaml',
        sharedInputs: [],
      },
    })
    workspace.manifestOf('b').description = 'edited'
    expect(await check(workspace, undefined, {
      dev: true,
      optional: false,
      production: false,
      ignoredWorkspaceStateSettings: ['dev', 'optional', 'production'],
    })).toMatchObject({ upToDate: true })
    const refreshed = loadWorkspaceState(workspace.workspaceDir)!
    expect(refreshed.settings.autoDedupe).toBe(true)
    expect(refreshed.settings.dev).toBe(false)
    expect(refreshed.settings.optional).toBe(true)
    expect(refreshed.settings.production).toBe(true)
  })

  it('holds a non-recursive command in a project to its workspace dependencies', async () => {
    const lockfile = lockfileWith({ a: 'link:../a' })
    lockfile.importers['packages/b' as ProjectId].specifiers = { a: 'workspace:*' }
    const workspace = await installedWorkspace(lockfile)
    await fs.rm(path.join(workspace.workspaceDir, 'node_modules/.pnpm-workspace-state-v1.json'))
    workspace.manifestOf('b').dependencies = { a: 'workspace:*' }
    await recordInstall(workspace, ['b'])
    expect(await check(workspace, undefined, { dir: workspace.projectDir('b') })).toMatchObject({
      upToDate: false,
      issue: 'Workspace package a has dependencies but was not installed',
    })

    // An npm: alias that resolves to a workspace package is also followed.
    workspace.manifestOf('b').dependencies = { aliased: 'npm:a@^1.0.0' }
    expect(await check(workspace, undefined, { dir: workspace.projectDir('b') })).toMatchObject({
      upToDate: false,
      issue: 'Workspace package a has dependencies but was not installed',
    })

    // A bare version range that does not match the workspace package version is not followed.
    workspace.manifestOf('b').dependencies = { a: '^2.0.0' }
    await writeWantedLockfile(workspace.workspaceDir, lockfileWith({ a: '2.0.0' }))
    await writeCurrentLockfile(path.join(workspace.workspaceDir, 'node_modules/.pnpm'), {
      ...lockfileWith({ a: '2.0.0' }),
      packages: { ['a@2.0.0' as DepPath]: { resolution: { integrity: 'sha512-a2' } } },
    })
    await updateWorkspaceState({
      allProjects: workspace.allProjects,
      workspaceDir: workspace.workspaceDir,
      pnpmfiles: [],
      settings: SETTINGS,
      configDependencies: CONFIG_DEPENDENCIES,
      filteredInstall: true,
      validation: {
        lockfileDir: workspace.workspaceDir,
        wantedLockfileName: 'pnpm-lock.yaml',
        sharedInputs: [],
        vouchedProjects: [workspace.projectDir('b')],
      },
    })
    expect(await check(workspace, undefined, { dir: workspace.projectDir('b') })).toMatchObject({ upToDate: true })

    // A non-recursive command in a project with a catalog or peer workspace dependency holds that dependency too.
    workspace.manifestOf('a').dependencies = { b: 'catalog:' }
    expect(await check(workspace, undefined, {
      dir: workspace.projectDir('a'),
      linkWorkspacePackages: true,
      catalogs: { default: { b: '^1.0.0' } },
    })).toMatchObject({ upToDate: false })

    delete workspace.manifestOf('a').dependencies
    workspace.manifestOf('a').peerDependencies = { b: 'workspace:*' }
    await fs.rm(path.join(workspace.projectDir('b'), 'node_modules'), { recursive: true, force: true })
    expect(await check(workspace, undefined, {
      dir: workspace.projectDir('a'),
    })).toMatchObject({ upToDate: false })

    // Backslash-separated workspace: and file: paths also reach their workspace dependency.
    delete workspace.manifestOf('a').peerDependencies
    workspace.manifestOf('a').dependencies = { b: 'workspace:..\\b' }
    expect(await check(workspace, undefined, {
      dir: workspace.projectDir('a'),
    })).toMatchObject({ upToDate: false })

    workspace.manifestOf('a').dependencies = { b: 'file:..\\b' }
    expect(await check(workspace, undefined, {
      dir: workspace.projectDir('a'),
    })).toMatchObject({ upToDate: false })
  })

  it('proves and refreshes per-project lockfiles when sharedWorkspaceLockfile is false', async () => {
    const workspace = await installedWorkspace(lockfileWith({ bar: '1.0.0' }))
    await fs.rm(path.join(workspace.workspaceDir, 'pnpm-lock.yaml'))
    await fs.rm(path.join(workspace.workspaceDir, 'node_modules/.pnpm'), { recursive: true, force: true })
    await Promise.all(([['a', 'foo'], ['b', 'bar']] as const).map(async ([name, dep]) => {
      const dir = workspace.projectDir(name)
      const projectLockfile: LockfileObject = {
        lockfileVersion: '9.0',
        settings: { autoInstallPeers: true, excludeLinksFromLockfile: false },
        importers: { ['.' as ProjectId]: { specifiers: { [dep]: '1.0.0' }, dependencies: { [dep]: '1.0.0' } } },
        packages: { [`${dep}@1.0.0` as DepPath]: { resolution: { integrity: `sha512-${dep}` } } },
      }
      await writeWantedLockfile(dir, projectLockfile)
      await writeCurrentLockfile(path.join(dir, 'node_modules/.pnpm'), projectLockfile)
    }))
    await updateWorkspaceState({
      allProjects: workspace.allProjects,
      workspaceDir: workspace.workspaceDir,
      pnpmfiles: [],
      settings: SETTINGS,
      configDependencies: CONFIG_DEPENDENCIES,
      filteredInstall: false,
      validation: {
        lockfileDir: workspace.workspaceDir,
        wantedLockfileName: 'pnpm-lock.yaml',
        sharedInputs: [],
        sharedWorkspaceLockfile: false,
      },
    })
    expect(await check(workspace, ['a'], { sharedWorkspaceLockfile: false })).toMatchObject({ upToDate: true })
    const manifestPath = path.join(workspace.projectDir('b'), 'package.json')
    const { mtime } = await fs.stat(manifestPath)
    workspace.manifestOf('b').dependencies = { bar: '1.0.0', foo: '1.0.0' }
    await fs.writeFile(manifestPath, JSON.stringify(workspace.manifestOf('b')))
    await fs.utimes(manifestPath, mtime, mtime)
    expect(await check(workspace, ['a'], { sharedWorkspaceLockfile: false })).toMatchObject({ upToDate: true })
    expect(await check(workspace, ['b'], { sharedWorkspaceLockfile: false })).toMatchObject({ upToDate: false })

    // An unreadable lockfile in an unselected project does not block a filtered check.
    const bLockfilePath = path.join(workspace.projectDir('b'), 'pnpm-lock.yaml')
    const { mtime: bLockfileMtime } = await fs.stat(bLockfilePath)
    await fs.rm(bLockfilePath)
    await fs.mkdir(bLockfilePath)
    await fs.utimes(bLockfilePath, bLockfileMtime, bLockfileMtime)
    expect(await check(workspace, ['a'], { sharedWorkspaceLockfile: false })).toMatchObject({ upToDate: true })

    // Under dedicated lockfiles with injection, a non-recursive check in `a`
    // skips injected `workspace:*` dependencies while still holding `link:` dependencies.
    await fs.rm(bLockfilePath, { recursive: true, force: true })
    await fs.rm(path.join(workspace.projectDir('b'), 'node_modules'), { recursive: true, force: true })
    const recordVouchedA = async (bSpec: string): Promise<void> => {
      workspace.manifestOf('a').dependencies = { foo: '1.0.0', b: bSpec }
      await updateWorkspaceState({
        allProjects: workspace.allProjects,
        workspaceDir: workspace.workspaceDir,
        pnpmfiles: [],
        settings: { ...SETTINGS, injectWorkspacePackages: true },
        configDependencies: CONFIG_DEPENDENCIES,
        filteredInstall: true,
        validation: {
          lockfileDir: workspace.workspaceDir,
          wantedLockfileName: 'pnpm-lock.yaml',
          sharedInputs: [],
          sharedWorkspaceLockfile: false,
          vouchedProjects: [workspace.projectDir('a')],
        },
      })
    }
    await recordVouchedA('workspace:*')
    expect(await check(workspace, undefined, {
      dir: workspace.projectDir('a'),
      injectWorkspacePackages: true,
      sharedWorkspaceLockfile: false,
    })).toMatchObject({ upToDate: true })

    await recordVouchedA('link:../b')
    expect(await check(workspace, undefined, {
      dir: workspace.projectDir('a'),
      injectWorkspacePackages: true,
      sharedWorkspaceLockfile: false,
    })).toMatchObject({
      upToDate: false,
      issue: 'Workspace package b has dependencies but does not have a modules directory',
    })
  })

  it('reports outdated when an importer catalog resolution disagrees with the lockfile catalog snapshot', async () => {
    const catalogs = { default: { bar: '^2.0.0' } }
    const lockfile = lockfileWith({ bar: '1.0.0' })
    lockfile.importers['packages/b' as ProjectId].specifiers = { bar: 'catalog:' }
    lockfile.catalogs = { default: { bar: { specifier: '^2.0.0', version: '2.0.0' } } }
    const workspace = await installedWorkspace(lockfile)
    workspace.manifestOf('b').dependencies = { bar: 'catalog:' }
    await updateWorkspaceState({
      allProjects: workspace.allProjects,
      workspaceDir: workspace.workspaceDir,
      pnpmfiles: [],
      settings: { ...SETTINGS, catalogs },
      configDependencies: CONFIG_DEPENDENCIES,
      filteredInstall: false,
      validation: {
        lockfileDir: workspace.workspaceDir,
        wantedLockfileName: 'pnpm-lock.yaml',
        sharedInputs: [],
      },
    })
    workspace.manifestOf('b').description = 'edited'
    await fs.writeFile(path.join(workspace.projectDir('b'), 'package.json'), JSON.stringify(workspace.manifestOf('b')))
    expect(await check(workspace, undefined, { catalogs })).toMatchObject({
      upToDate: false,
      issue: expect.stringContaining('does not satisfy project of id packages/b'),
    })
  })

  it('checks a non-workspace root project against a custom lockfileDir', async () => {
    const lockfileDir = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-custom-lockfile-dir-')))
    workspaces.push(lockfileDir)
    const projectDir = path.join(lockfileDir, 'app')
    await fs.mkdir(projectDir, { recursive: true })
    const manifest: ProjectManifest = { name: 'app', version: '1.0.0', dependencies: { foo: '1.0.0' } }
    await fs.writeFile(path.join(projectDir, 'package.json'), JSON.stringify(manifest))
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      settings: { autoInstallPeers: true, excludeLinksFromLockfile: false },
      importers: {
        ['app' as ProjectId]: { specifiers: { foo: '1.0.0' }, dependencies: { foo: '1.0.0' } },
      },
      packages: {
        ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-foo' } },
      },
    }
    await writeWantedLockfile(lockfileDir, lockfile)
    await writeCurrentLockfile(path.join(lockfileDir, 'node_modules/.pnpm'), lockfile)
    await updateWorkspaceState({
      allProjects: [{ rootDir: projectDir as ProjectRootDir, manifest }],
      workspaceDir: lockfileDir,
      pnpmfiles: [],
      settings: SETTINGS,
      configDependencies: CONFIG_DEPENDENCIES,
      filteredInstall: false,
      validation: {
        lockfileDir,
        wantedLockfileName: 'pnpm-lock.yaml',
        sharedInputs: [],
      },
    })
    expect(await checkDepsStatus({
      ...SETTINGS,
      lockfileDir,
      rootProjectManifestDir: projectDir,
      rootProjectManifest: manifest,
      pnpmfile: [],
      configDependencies: CONFIG_DEPENDENCIES,
    })).toMatchObject({ upToDate: true })

    manifest.description = 'edited'
    await fs.writeFile(path.join(projectDir, 'package.json'), JSON.stringify(manifest))
    expect(await checkDepsStatus({
      ...SETTINGS,
      lockfileDir,
      rootProjectManifestDir: projectDir,
      rootProjectManifest: manifest,
      pnpmfile: [],
      configDependencies: CONFIG_DEPENDENCIES,
    })).toMatchObject({ upToDate: true })

    const refreshed = loadWorkspaceState(lockfileDir)!
    expect(refreshed.configDependencies).toStrictEqual(CONFIG_DEPENDENCIES)
    expect(refreshed.projects[projectDir as ProjectRootDir]).toMatchObject({ name: 'app', version: '1.0.0' })
    expect(refreshed.validation?.manifests?.app).toBeDefined()
    expect(await checkDepsStatus({
      ...SETTINGS,
      lockfileDir,
      rootProjectManifestDir: projectDir,
      rootProjectManifest: manifest,
      pnpmfile: [],
      configDependencies: CONFIG_DEPENDENCIES,
    })).toMatchObject({ upToDate: true })
  })
})

/**
 * Waits until the filesystem clock in `dir` passes `ns`, so a record made now
 * can be trusted.
 */
function waitForFilesystemClockPast (dir: string, ns: number): void {
  const probe = path.join(dir, '.clock-probe')
  do {
    writeFileSync(probe, '')
  } while (Number(statSync(probe, { bigint: true }).mtimeNs) <= ns)
  rmSync(probe)
}
