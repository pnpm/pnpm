import type { PnpmSettings, ProjectManifest } from '@pnpm/types'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'

export interface WriteSettingsOptions {
  updatedSettings?: PnpmSettings
  updatedOverrides?: Record<string, string>
  updatedAuditIgnoreGhsas?: string[]
  addedMinimumReleaseAgeExcludes?: string[]
  deletedLegacyKeys?: string[]
  rootProjectManifest?: ProjectManifest
  rootProjectManifestDir: string
  workspaceDir: string
}

export async function writeSettings (opts: WriteSettingsOptions): Promise<void> {
  await updateWorkspaceManifest(opts.workspaceDir, {
    updatedFields: opts.updatedSettings,
    updatedOverrides: opts.updatedOverrides,
    updatedAuditIgnoreGhsas: opts.updatedAuditIgnoreGhsas,
    addedMinimumReleaseAgeExcludes: opts.addedMinimumReleaseAgeExcludes,
    deletedLegacyKeys: opts.deletedLegacyKeys,
  })
}
