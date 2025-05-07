import path from 'path'
import normalizePath from 'normalize-path'
import { writeSettings, type WriteSettingsOptions } from '@pnpm/config.config-writer'

export async function updatePatchedDependencies (
  patchedDependencies: Record<string, string>,
  opts: Omit<WriteSettingsOptions, 'updatedSettings'>
): Promise<void> {
  const workspaceDir = opts.workspaceDir ?? opts.rootProjectManifestDir
  for (const [patchName, patchPath] of Object.entries(patchedDependencies)) {
    if (path.isAbsolute(patchPath)) {
      patchedDependencies[patchName] = normalizePath(path.relative(workspaceDir, patchPath))
    }
  }
  await writeSettings({
    ...opts,
    workspaceDir,
    updatedSettings: {
      patchedDependencies: Object.keys(patchedDependencies).length ? patchedDependencies : undefined,
    },
  })
}
