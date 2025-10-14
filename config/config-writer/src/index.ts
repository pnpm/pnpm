import { type ProjectManifest, type PnpmSettings } from '@pnpm/types'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'

export interface WriteSettingsOptions {
  updatedSettings: PnpmSettings
  rootProjectManifest?: ProjectManifest
  rootProjectManifestDir: string
  workspaceDir: string
}

export async function writeSettings (opts: WriteSettingsOptions): Promise<void> {
  await updateWorkspaceManifest(opts.workspaceDir, {
    updatedFields: opts.updatedSettings,
  })
}
