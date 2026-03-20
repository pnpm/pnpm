import { writeSettings } from '@pnpm/config.writer'
import {
  dedupePackageNamesFromIgnoredBuilds,
  IgnoredBuildsError,
} from '@pnpm/installing.deps-installer'
import type { IgnoredBuilds } from '@pnpm/types'

export async function handleIgnoredBuilds (
  opts: {
    allowBuilds?: Record<string, boolean | string>
    rootProjectManifestDir?: string
    workspaceDir?: string
    strictDepBuilds?: boolean
  },
  ignoredBuilds: IgnoredBuilds | undefined
): Promise<void> {
  if (!ignoredBuilds?.size) return
  const packageNames = dedupePackageNamesFromIgnoredBuilds(ignoredBuilds)
  const newEntries: Record<string, string> = {}
  for (const name of packageNames) {
    if (opts.allowBuilds?.[name] == null) {
      newEntries[name] = 'true|false'
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
  if (opts.strictDepBuilds) {
    throw new IgnoredBuildsError(ignoredBuilds)
  }
}
