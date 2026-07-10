import type { LicensesConfig, ProjectManifest } from '@pnpm/types'

import { editLicenseList } from './editLicenseList.js'
import type { LicensesCommandResult } from './LicensesCommandResult.js'

export interface LicensesDisallowOptions {
  rootProjectManifest?: ProjectManifest
  rootProjectManifestDir: string
  workspaceDir?: string
  licenses?: LicensesConfig
}

export async function licensesDisallow (opts: LicensesDisallowOptions, licenses: string[]): Promise<LicensesCommandResult> {
  return editLicenseList(opts, licenses, 'disallowed')
}
