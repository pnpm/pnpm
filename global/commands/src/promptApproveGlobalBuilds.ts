import path from 'node:path'

import type { CommandHandlerMap } from '@pnpm/cli.command'
import type { IgnoredBuilds } from '@pnpm/types'

export interface PromptApproveGlobalBuildsOptions {
  globalPkgDir?: string
  installDir: string
  ignoredBuilds: IgnoredBuilds | undefined
  allowBuilds: Record<string, string | boolean>
  /** Inherited config opts from the global add/update handler. */
  inheritedOpts: object
}

/**
 * If the previous global install left builds awaiting approval, run the
 * interactive `approve-builds` flow against the install directory.
 *
 * `settingsDir` points at the global packages directory so the resulting
 * allowBuilds update lands in its pnpm-workspace.yaml. `workspaceDir` is
 * intentionally not set — otherwise the install that approve-builds runs in
 * GVS mode would treat the global packages dir as a workspace and discover
 * sibling install directories as workspace projects.
 */
export async function promptApproveGlobalBuilds (
  opts: PromptApproveGlobalBuildsOptions,
  commands: CommandHandlerMap
): Promise<void> {
  if (!opts.ignoredBuilds?.size || !process.stdin.isTTY) return
  await commands['approve-builds']({
    ...opts.inheritedOpts,
    modulesDir: path.join(opts.installDir, 'node_modules'),
    dir: opts.installDir,
    lockfileDir: opts.installDir,
    rootProjectManifest: undefined,
    rootProjectManifestDir: opts.installDir,
    settingsDir: opts.globalPkgDir,
    global: false,
    pending: false,
    allowBuilds: opts.allowBuilds,
  }, [], commands)
}
