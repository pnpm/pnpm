import path from 'node:path'

import { writeSettings, type WriteSettingsOptions } from '@pnpm/config.writer'
import normalizePath from 'normalize-path'

export async function updatePatchedDependencies (
  patchedDependencies: Record<string, string>,
  opts: Omit<WriteSettingsOptions, 'updatedSettings'>
): Promise<void> {
  const workspaceDir = opts.workspaceDir ?? opts.rootProjectManifestDir
  const relativePatchedDependencies: Record<string, string> = {}
  for (const [patchName, patchPath] of Object.entries(patchedDependencies)) {
    relativePatchedDependencies[patchName] = path.isAbsolute(patchPath)
      ? normalizePath(path.relative(workspaceDir, patchPath))
      : patchPath
  }
  await writeSettings({
    ...opts,
    workspaceDir,
    updatedSettings: {
      patchedDependencies: Object.keys(relativePatchedDependencies).length ? relativePatchedDependencies : undefined,
    },
  })
}
