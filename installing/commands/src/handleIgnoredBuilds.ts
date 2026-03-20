import { writeSettings } from '@pnpm/config.writer'
import {
  dedupePackageNamesFromIgnoredBuilds,
  IgnoredBuildsError,
} from '@pnpm/installing.deps-installer'
import type { IgnoredBuilds } from '@pnpm/types'

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

export async function writeIgnoredBuildsToAllowBuilds (
  opts: Pick<HandleIgnoredBuildsOpts, 'allowBuilds' | 'rootProjectManifestDir' | 'workspaceDir'>,
  ignoredBuilds: IgnoredBuilds
): Promise<void> {
  const packageNames = dedupePackageNamesFromIgnoredBuilds(ignoredBuilds)
  const newEntries: Record<string, string> = {}
  for (const name of packageNames) {
    if (opts.allowBuilds?.[name] == null) {
      newEntries[name] = 'set this to true or false'
    }
  }
  if (Object.keys(newEntries).length && opts.rootProjectManifestDir) {
    await writeSettings({
      rootProjectManifestDir: opts.rootProjectManifestDir,
      workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
      updatedSettings: {
        allowBuilds: { ...opts.allowBuilds, ...newEntries },
      },
    })
  }
}
