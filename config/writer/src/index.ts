import type { PnpmSettings, ProjectManifest } from '@pnpm/types'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'

export interface WriteSettingsOptions {
  updatedSettings?: PnpmSettings
  updatedOverrides?: Record<string, string>
  rootProjectManifest?: ProjectManifest
  rootProjectManifestDir: string
  workspaceDir: string
}

export async function writeSettings (opts: WriteSettingsOptions): Promise<void> {
  await updateWorkspaceManifest(opts.workspaceDir, {
    updatedFields: opts.updatedSettings,
    updatedOverrides: opts.updatedOverrides,
  })
}
