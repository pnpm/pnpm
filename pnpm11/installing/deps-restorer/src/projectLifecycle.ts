import type { DependenciesGraphNode } from '@pnpm/deps.graph-builder'
import {
  POST_UNINSTALL_STAGES,
  PROJECT_INSTALL_STAGES,
  PROJECT_LIFECYCLE_STAGES,
  runLifecycleHooksConcurrently,
  type RunLifecycleHooksConcurrentlyOptions,
} from '@pnpm/exec.lifecycle'
import type { DepPath, ProjectId, ProjectManifest } from '@pnpm/types'

import type { HeadlessContext } from './context.js'
import type { extendProjectsWithTargetDirs } from './extendProjectsWithTargetDirs.js'
import { addPackageMapOption } from './packageMap.js'
import type { HeadlessOptions } from './types.js'

type ProjectToBeBuilt = ReturnType<typeof extendProjectsWithTargetDirs<HeadlessContext['selectedProjects'][number]>>[number]

export function reconcilePendingBuilds ({ opts, projectsRunningScripts, wantedLockfile }: HeadlessContext, depNodes: DependenciesGraphNode[]): void {
  // Reconcile in every mode, not only when scripts are ignored: an entry
  // whose package the wanted lockfile no longer records would otherwise
  // stay pending forever.
  opts.pendingBuilds = opts.pendingBuilds
    .filter((id) => wantedLockfile.packages?.[id as DepPath] != null || wantedLockfile.importers[id as ProjectId] != null)
  if (!opts.ignoreScripts) return
  for (const { id, manifest } of projectsRunningScripts) {
    if (hasInstallScripts(manifest)) {
      opts.pendingBuilds.push(id)
    }
  }
  opts.pendingBuilds = Array.from(new Set(
    opts.pendingBuilds.concat(
      depNodes
        .filter(({ requiresBuild }) => requiresBuild)
        .map(({ depPath }) => depPath)
    )
  ))
}

function hasInstallScripts (manifest: ProjectManifest | undefined): boolean {
  if (manifest?.scripts == null) return false
  return Boolean(manifest.scripts.preinstall ?? manifest.scripts.prepublish ??
    manifest.scripts.install ??
    manifest.scripts.postinstall ??
    manifest.scripts.prepare)
}

export async function runProjectLifecycleScripts (
  ctx: HeadlessContext,
  projectsToBeBuilt: ProjectToBeBuilt[],
  shouldWritePackageMap: boolean
): Promise<void> {
  const { opts } = ctx
  if (opts.ignoreScripts || opts.ignorePackageManifest || ctx.skipPostImportLinking) return
  const scriptsOpts = createScriptsOptions(opts)
  if (opts.nodeExperimentalPackageMap && shouldWritePackageMap) {
    scriptsOpts.extraEnv = addPackageMapOption(scriptsOpts.extraEnv, ctx.rootModulesDir)
  }
  // The projects' own lifecycle scripts import dependency code linked from
  // the lockfile, so they are held to the same gate as dependency builds —
  // also on the `enableModulesDir: false` path that skips buildModules.
  await opts.verifyLockfile?.()
  await runLifecycleHooksConcurrently({
    childConcurrency: opts.childConcurrency ?? 5,
    importers: selectProjectsRunningLifecycleScripts(ctx, projectsToBeBuilt),
    opts: scriptsOpts,
    projectDependencies: opts.projectDependencies,
    projectWithPreinstallRan: opts.rootProjectPreinstallRan ? opts.lockfileDir : undefined,
    stages: (opts.deploy || opts.include?.devDependencies === false)
      ? PROJECT_INSTALL_STAGES
      : PROJECT_LIFECYCLE_STAGES,
  })
}

function createScriptsOptions (opts: HeadlessOptions): RunLifecycleHooksConcurrentlyOptions & Pick<HeadlessOptions, 'configByUri'> {
  return {
    optional: false,
    extraBinPaths: opts.extraBinPaths,
    extendNodePath: opts.extendNodePath,
    extraNodePaths: opts.extraNodePaths,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    extraEnv: opts.extraEnv,
    configByUri: opts.configByUri,
    resolveSymlinksInInjectedDirs: opts.resolveSymlinksInInjectedDirs,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    stdio: opts.ownLifecycleHooksStdio ?? 'inherit',
    storeController: opts.storeController,
    unsafePerm: opts.unsafePerm || false,
    userAgent: opts.userAgent,
  }
}

function selectProjectsRunningLifecycleScripts (
  { opts, projectDirsRunningScripts }: HeadlessContext,
  projectsToBeBuilt: ProjectToBeBuilt[]
): ProjectToBeBuilt[] {
  const projectDirsRunningUninstallScripts = new Set(opts.projectDirsRunningUninstallScripts)
  const projectDirsRunningInstallOnlyScripts = new Set(opts.projectDirsRunningInstallOnlyScripts)
  return projectsToBeBuilt.flatMap((project) => {
    if (projectDirsRunningInstallOnlyScripts.has(project.rootDir)) return [{ ...project, stages: PROJECT_INSTALL_STAGES }]
    if (projectDirsRunningScripts.has(project.rootDir)) return [project]
    if (projectDirsRunningUninstallScripts.has(project.rootDir)) return [{ ...project, stages: POST_UNINSTALL_STAGES }]
    return []
  })
}
