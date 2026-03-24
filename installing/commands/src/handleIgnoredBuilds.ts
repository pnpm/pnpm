import { writeSettings } from '@pnpm/config.writer'
import { parse } from '@pnpm/deps.path'
import {
  IgnoredBuildsError,
} from '@pnpm/installing.deps-installer'
import type { IgnoredBuilds } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

export interface HandleIgnoredBuildsOpts {
  allowBuilds?: Record<string, boolean | string>
  rootProjectManifestDir?: string
  workspaceDir?: string
  strictDepBuilds?: boolean
}

export async function handleIgnoredBuilds (
  opts: HandleIgnoredBuildsOpts,
  ignoredBuilds: IgnoredBuilds | undefined
): Promise<void> {
  if (!ignoredBuilds?.size) return
  await writeIgnoredBuildsToAllowBuilds(opts, ignoredBuilds)
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
      newEntries[name] = 'set this to true or false'
    }
  }
  if (Object.keys(newEntries).length && opts.rootProjectManifestDir) {
    // Read the current allowBuilds from pnpm-workspace.yaml rather than
    // using the runtime config, which may include defaults from the
    // trusted deps list that should not be persisted.
    const workspaceDir = opts.workspaceDir ?? opts.rootProjectManifestDir
    const workspaceManifest = await readWorkspaceManifest(workspaceDir)
    const currentAllowBuilds = workspaceManifest?.allowBuilds ?? {}
    await writeSettings({
      rootProjectManifestDir: opts.rootProjectManifestDir,
      workspaceDir,
      updatedSettings: {
        allowBuilds: { ...currentAllowBuilds, ...newEntries },
      },
    })
  }
}

function packageNamesFromIgnoredBuilds (ignoredBuilds: IgnoredBuilds): string[] {
  return Array.from(new Set(Array.from(ignoredBuilds).map((dp) => parse(dp).name ?? dp))).sort(lexCompare)
}
