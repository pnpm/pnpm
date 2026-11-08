import { allowBuildKeyFromIgnoredBuild, UNDECIDED_ALLOW_BUILD } from '@pnpm/building.policy'
import { writeSettings } from '@pnpm/config.writer'
import {
  IgnoredBuildsError,
} from '@pnpm/installing.deps-installer'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { IgnoredBuilds } from '@pnpm/types'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { isCI } from 'ci-info'

export interface HandleIgnoredBuildsOpts {
  allowBuilds?: Record<string, boolean | string>
  ci?: boolean
  ignoreWorkspace?: boolean
  rootProjectManifestDir?: string
  workspaceDir?: string
  strictDepBuilds?: boolean
}

export async function handleIgnoredBuilds (
  opts: HandleIgnoredBuildsOpts,
  ignoredBuilds: IgnoredBuilds | undefined
): Promise<void> {
  if (!ignoredBuilds?.size) return
  // Nobody is at the terminal to edit a placeholder in CI or under a
  // dependency-update bot, and it would land in the committed workspace manifest.
  const canPrompt = !(opts.ci ?? isCI) && Boolean(process.stdin.isTTY)
  if (canPrompt && !opts.ignoreWorkspace) {
    await writeIgnoredBuildsToAllowBuilds(opts, ignoredBuilds)
  }
  if (opts.strictDepBuilds) {
    throw new IgnoredBuildsError(ignoredBuilds)
  }
}

async function writeIgnoredBuildsToAllowBuilds (
  opts: Pick<HandleIgnoredBuildsOpts, 'allowBuilds' | 'rootProjectManifestDir' | 'workspaceDir'>,
  ignoredBuilds: IgnoredBuilds
): Promise<void> {
  const packageNames = packageNamesFromIgnoredBuilds(ignoredBuilds)
  const newEntries: Record<string, string> = {}
  for (const name of packageNames) {
    if (opts.allowBuilds?.[name] == null) {
      newEntries[name] = UNDECIDED_ALLOW_BUILD
    }
  }
  if (Object.keys(newEntries).length && opts.rootProjectManifestDir) {
    const workspaceDir = opts.workspaceDir ?? opts.rootProjectManifestDir
    // Merged into the file's own entries: `opts.allowBuilds` also holds
    // entries from other config sources and approvals made for this run only.
    const workspaceManifest = await readWorkspaceManifest(workspaceDir)
    await writeSettings({
      rootProjectManifestDir: opts.rootProjectManifestDir,
      workspaceDir,
      updatedSettings: {
        allowBuilds: { ...workspaceManifest?.allowBuilds, ...newEntries },
      },
    })
  }
}

function packageNamesFromIgnoredBuilds (ignoredBuilds: IgnoredBuilds): string[] {
  return Array.from(new Set(Array.from(ignoredBuilds).map(allowBuildKeyFromIgnoredBuild))).sort(lexCompare)
}
