import path from 'node:path'

import { makeProjectNodePathOption, runLifecycleHook, type RunLifecycleHookOptions } from '@pnpm/exec.lifecycle'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'

import { extendOptions, type InstallOptions, type ProcessedInstallOptions } from './extendInstallOptions.js'
import { DEV_PREINSTALL, installRunsDevPreinstall } from './installPredicates.js'

export type RootHookOptionsSource = Partial<Pick<ProcessedInstallOptions,
| 'extendNodePath'
| 'extraBinPaths'
| 'extraEnv'
| 'modulesDir'
| 'ownLifecycleHooksStdio'
| 'preferSymlinkedExecutables'
| 'scriptShell'
| 'scriptsPrependNodePath'
| 'shellEmulator'
| 'unsafePerm'
| 'userAgent'
>>

/**
 * The options for a root project hook that runs before the install has
 * opened its context.
 */
export async function createRootHookOptions (lockfileDir: string, opts: RootHookOptionsSource): Promise<RunLifecycleHookOptions> {
  const rootModulesDir = path.join(lockfileDir, opts.modulesDir ?? 'node_modules')
  return {
    depPath: lockfileDir,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: {
      ...opts.extraEnv,
      ...await makeProjectNodePathOption({ modulesDir: rootModulesDir, rootDir: lockfileDir }, opts),
    },
    pkgRoot: lockfileDir,
    rootModulesDir,
    wdBinDir: path.join(rootModulesDir, '.bin'),
    scriptShell: opts.scriptShell,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    shellEmulator: opts.shellEmulator,
    stdio: opts.ownLifecycleHooksStdio,
    unsafePerm: opts.unsafePerm || false,
    userAgent: opts.userAgent,
  }
}

/**
 * Runs the workspace root's `pnpm:devPreinstall` for installs that run no
 * scripts of their own, such as the per-project installs of a workspace
 * without a shared lockfile.
 */
export async function runDevPreinstall (workspaceDir: string, maybeOpts: InstallOptions): Promise<void> {
  const opts = extendOptions({ ...maybeOpts, lockfileDir: workspaceDir })
  if (!installRunsDevPreinstall(opts)) return
  const rootProjectManifest = await safeReadProjectManifestOnly(workspaceDir)
  if (!rootProjectManifest?.scripts?.[DEV_PREINSTALL]) return
  await runLifecycleHook(DEV_PREINSTALL, rootProjectManifest, await createRootHookOptions(workspaceDir, opts))
}
