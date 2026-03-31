import { writeSettings } from '@pnpm/config.writer'
import { extractLicenseIds } from '@pnpm/deps.compliance.license-checker'
import { PnpmError } from '@pnpm/error'
import type { LicensesConfig, ProjectManifest } from '@pnpm/types'

import type { LicensesCommandResult } from './LicensesCommandResult.js'

export interface LicensesDisallowOptions {
  rootProjectManifest?: ProjectManifest
  rootProjectManifestDir: string
  workspaceDir?: string
  licenses?: LicensesConfig
}

export async function licensesDisallow (
  opts: LicensesDisallowOptions,
  licenses: string[]
): Promise<LicensesCommandResult> {
  if (licenses.length === 0) {
    throw new PnpmError(
      'LICENSES_DISALLOW_NO_ARGS',
      'Please specify at least one license to disallow'
    )
  }

  if (!opts.workspaceDir) {
    throw new PnpmError(
      'LICENSES_NO_WORKSPACE',
      'Cannot modify license settings outside of a workspace'
    )
  }

  const currentDisallowed = opts.licenses?.disallowed ?? []
  const currentAllowed = opts.licenses?.allowed ?? []
  const newDisallowed = [...new Set([...currentDisallowed, ...licenses])]
  const newAllowed = currentAllowed.filter((l) => !licenses.includes(l))

  const updatedConfig: LicensesConfig = {
    ...opts.licenses,
    disallowed: newDisallowed,
  }
  if (newAllowed.length !== currentAllowed.length) {
    updatedConfig.allowed = newAllowed.length > 0 ? newAllowed : undefined
  }

  await writeSettings({
    ...opts,
    workspaceDir: opts.workspaceDir,
    updatedSettings: {
      licenses: updatedConfig,
    },
  })

  const added = licenses.filter((l) => !currentDisallowed.includes(l))
  const lines: string[] = []
  if (added.length > 0) {
    lines.push(`Added to disallowed licenses: ${added.join(', ')}`)
  } else {
    lines.push('All specified licenses are already in the disallowed list')
  }

  const removedFromAllowed = currentAllowed.filter((l) => licenses.includes(l))
  if (removedFromAllowed.length > 0) {
    lines.push(`Removed from allowed licenses: ${removedFromAllowed.join(', ')}`)
  }

  const unrecognized = licenses.filter((l) => extractLicenseIds(l).length === 0)
  if (unrecognized.length > 0) {
    lines.push(`Note: could not be parsed as SPDX expressions; will be matched literally: ${unrecognized.join(', ')}`)
  }

  return { output: lines.join('\n'), exitCode: 0 }
}
