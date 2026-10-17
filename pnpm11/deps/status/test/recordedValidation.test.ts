import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterEach, describe, expect, it } from '@jest/globals'
import { checkDepsStatus, type CheckDepsStatusOptions } from '@pnpm/deps.status'
import { type LockfileObject, writeCurrentLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { DepPath, Project, ProjectId, ProjectManifest, ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
import { updateWorkspaceState } from '@pnpm/workspace.state'

const SETTINGS = {
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

/** Record the state an install leaves, vouching for `vouched` projects only when set. */
async function recordInstall (workspace: RecordedWorkspace, vouched?: string[]): Promise<void> {
  await updateWorkspaceState({
    allProjects: workspace.allProjects,
    workspaceDir: workspace.workspaceDir,
    pnpmfiles: [],
    settings: SETTINGS,
    filteredInstall: vouched != null,
    validation: {
      lockfileDir: workspace.workspaceDir,
      wantedLockfileName: 'pnpm-lock.yaml',
      sharedInputs: [],
      vouchedProjects: vouched?.map(workspace.projectDir),
    },
  })
}

function check (workspace: RecordedWorkspace, selected?: string[]): ReturnType<typeof checkDepsStatus> {
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
    selectedProjectsGraph,
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
})
