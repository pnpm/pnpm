import { runLifecycleHook, type RunLifecycleHookOptions } from '@pnpm/exec.lifecycle'
import type { DependencyManifest, ProjectManifest } from '@pnpm/types'
import {
  injectedEditDirs,
  type InjectedEditWatch,
  syncInjectedDeps,
  watchInjectedEdits,
} from '@pnpm/workspace.injected-deps-syncer'

export interface RunScriptOptions {
  enablePrePostScripts: boolean
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
  const located = await injectedEditDirs({
    pkgName: opts.manifest.name,
    pkgRootDir: opts.lifecycleOpts.pkgRoot,
    workspaceDir: opts.runScriptOptions.workspaceDir,
  })
  if (located == null) return undefined
  return watchInjectedEdits(located.sourceDir, located.targetDirs)
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
