import type { CommandHandlerMap } from '@pnpm/cli.command'
import type { IgnoredBuilds } from '@pnpm/types'

export interface PromptApproveGlobalBuildsOptions {
  globalPkgDir: string
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
 * allowBuilds update lands in its pnpm-workspace.yaml. The
 * workspace-context fields (`workspaceDir`, `allProjects`,
 * `selectedProjectsGraph`, `workspacePackagePatterns`) are explicitly
 * cleared so that the install run by approve-builds in GVS mode operates
 * only on the install directory — otherwise it would treat the global
 * packages dir as a workspace and discover sibling install directories as
 * workspace projects.
 *
 * `modulesDir` is left undefined so that downstream consumers compute it
 * relative to `lockfileDir`. Passing an absolute value here would be
 * forwarded as-is to `install.handler`, which treats `modulesDir` as a
 * path relative to `lockfileDir` and joins it again — producing a
 * doubled path on Windows (path.join does not collapse an embedded
 * absolute path).
 */
export async function promptApproveGlobalBuilds (
  opts: PromptApproveGlobalBuildsOptions,
  commands: CommandHandlerMap
): Promise<void> {
  if (!opts.ignoredBuilds?.size || !process.stdin.isTTY) return
  await commands['approve-builds']({
    ...opts.inheritedOpts,
    workspaceDir: undefined,
    allProjects: undefined,
    selectedProjectsGraph: undefined,
    workspacePackagePatterns: undefined,
    modulesDir: undefined,
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
