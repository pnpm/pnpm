import { writeSettings } from '@pnpm/config.writer'
import { parse } from '@pnpm/deps.path'
import {
  IgnoredBuildsError,
} from '@pnpm/installing.deps-installer'
import type { IgnoredBuilds } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'

export interface HandleIgnoredBuildsOpts {
  allowBuilds?: Record<string, boolean | string>
  userAllowBuilds?: Record<string, boolean | string>
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
  opts: Pick<HandleIgnoredBuildsOpts, 'allowBuilds' | 'userAllowBuilds' | 'rootProjectManifestDir' | 'workspaceDir'>,
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
    // Use userAllowBuilds (without defaults from the trusted deps list)
    // to avoid persisting the default list into pnpm-workspace.yaml.
    const allowBuildsToWrite = opts.userAllowBuilds ?? opts.allowBuilds
    await writeSettings({
      rootProjectManifestDir: opts.rootProjectManifestDir,
      workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
      updatedSettings: {
        allowBuilds: { ...allowBuildsToWrite, ...newEntries },
      },
    })
  }
}

function packageNamesFromIgnoredBuilds (ignoredBuilds: IgnoredBuilds): string[] {
  return Array.from(new Set(Array.from(ignoredBuilds).map((dp) => parse(dp).name ?? dp))).sort(lexCompare)
}
