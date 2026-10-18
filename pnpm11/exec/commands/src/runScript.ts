import { runLifecycleHook, type RunLifecycleHookOptions } from '@pnpm/exec.lifecycle'
import type { DependencyManifest, ProjectManifest } from '@pnpm/types'
import {
  injectedEditDirs,
  type InjectedEditWatch,
  type ProjectLockfiles,
  syncInjectedDeps,
  watchInjectedEdits,
} from '@pnpm/workspace.injected-deps-syncer'

export interface RunScriptOptions {
  deployAllFiles?: boolean
  enablePrePostScripts: boolean
  /** See `SyncInjectedDepsOptions.projectLockfiles`. */
  projectLockfiles?: ProjectLockfiles
  syncInjectedDepsAfterScripts: string[] | undefined
  workspaceDir: string | undefined
}

export interface RunScriptContext {
  manifest: ProjectManifest
  lifecycleOpts: RunLifecycleHookOptions
  runScriptOptions: RunScriptOptions
  passedThruArgs: string[]
}

export async function runScript (opts: RunScriptContext, scriptName: string): Promise<void> {
  const syncAfter = opts.runScriptOptions.syncInjectedDepsAfterScripts?.includes(scriptName) === true
  const watch = syncAfter ? await startInjectedEditWatch(opts) : undefined
  try {
    await runScriptStages(opts, scriptName)
  } finally {
    await watch?.stop()
  }
  if (syncAfter) {
    await syncInjectedDeps({
      pkgName: opts.manifest.name,
      pkgRootDir: opts.lifecycleOpts.pkgRoot,
      workspaceDir: opts.runScriptOptions.workspaceDir,
      // Read before the script ran, so a bin it drops can still be named.
      manifestBeforeScripts: opts.manifest as DependencyManifest,
      includeOnlyPackageFiles: !opts.runScriptOptions.deployAllFiles,
      projectLockfiles: opts.runScriptOptions.projectLockfiles,
    })
  }
}

async function runScriptStages (opts: RunScriptContext, scriptName: string): Promise<void> {
  const stages = getRunScriptStages(opts.manifest, scriptName, opts.runScriptOptions.enablePrePostScripts)
  if (stages.length === 0) {
    await runLifecycleHook(scriptName, opts.manifest, { ...opts.lifecycleOpts, args: opts.passedThruArgs })
    return
  }
  for (const stage of stages) {
    await runLifecycleHook(stage.name, opts.manifest, stage.name === scriptName // eslint-disable-line no-await-in-loop -- pre, main, and post scripts run in order
      ? { ...opts.lifecycleOpts, args: opts.passedThruArgs }
      : opts.lifecycleOpts)
  }
}

async function startInjectedEditWatch (opts: {
  manifest: ProjectManifest
  lifecycleOpts: RunLifecycleHookOptions
  runScriptOptions: RunScriptOptions
}): Promise<InjectedEditWatch | undefined> {
  const sources = await injectedEditDirs({
    pkgName: opts.manifest.name,
    pkgRootDir: opts.lifecycleOpts.pkgRoot,
    workspaceDir: opts.runScriptOptions.workspaceDir,
    manifestBeforeScripts: opts.manifest as DependencyManifest,
    projectLockfiles: opts.runScriptOptions.projectLockfiles,
  })
  if (sources.length === 0) return undefined
  const watches = sources.map(({ sourceDir, targetDirs }) => watchInjectedEdits(sourceDir, targetDirs, !opts.runScriptOptions.deployAllFiles))
  return {
    stop: async () => {
      await Promise.all(watches.map(async (watch) => watch.stop()))
    },
  }
}

/** `projectLockfiles` for the run's settings: set only when the workspace has no shared lockfile. */
export function projectLockfilesOf (opts: {
  modulesDir?: string
  modulesDirsByProjectName?: Record<string, string>
  sharedWorkspaceLockfile?: boolean
  workspacePackagePatterns?: string[]
}): ProjectLockfiles | undefined {
  if (opts.sharedWorkspaceLockfile !== false) return undefined
  return {
    modulesDir: opts.modulesDir,
    modulesDirsByProjectName: opts.modulesDirsByProjectName,
    workspacePackagePatterns: opts.workspacePackagePatterns,
  }
}

export function getRunScriptCommands (
  manifest: ProjectManifest,
  scriptName: string,
  enablePrePostScripts: boolean
): string[] {
  return getRunScriptStages(manifest, scriptName, enablePrePostScripts).map(({ command }) => command)
}

function getRunScriptStages (
  manifest: ProjectManifest,
  scriptName: string,
  enablePrePostScripts: boolean
): Array<{ name: string, command: string }> {
  const scripts = manifest.scripts ?? {}
  const main = scripts[scriptName]
  if (main == null) return []
  const stages = [{ name: scriptName, command: main }]
  if (!enablePrePostScripts) return stages
  const pre = `pre${scriptName}`
  const post = `post${scriptName}`
  if (scripts[pre] && !main.includes(pre)) stages.unshift({ name: pre, command: scripts[pre] })
  if (scripts[post] && !main.includes(post)) stages.push({ name: post, command: scripts[post] })
  return stages
}
