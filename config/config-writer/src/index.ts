import { type ProjectManifest, type PnpmSettings } from '@pnpm/types'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import equals from 'ramda/src/equals'

export async function writeSettings (opts: {
  updatedSettings: PnpmSettings
  rootProjectManifest?: ProjectManifest
  rootProjectManifestDir: string
  workspaceDir: string
}): Promise<void> {
  if (opts.rootProjectManifest?.pnpm != null) {
    const { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.rootProjectManifestDir)
    if (manifest) {
      manifest.pnpm ??= {}
      let shouldBeUpdated = false
      for (const [key, value] of Object.entries(opts.updatedSettings)) {
        if (!equals(manifest.pnpm[key as keyof PnpmSettings], value)) {
          shouldBeUpdated = true
          if (value == null) {
            delete manifest.pnpm[key as keyof PnpmSettings]
          } else {
            manifest.pnpm[key as keyof PnpmSettings] = value
          }
        }
      }
      if (Object.keys(manifest.pnpm).length === 0) {
        delete manifest.pnpm
      }
      if (shouldBeUpdated) {
        await writeProjectManifest(manifest)
      }
      return
    }
  }
  await updateWorkspaceManifest(opts.workspaceDir, opts.updatedSettings)
}
