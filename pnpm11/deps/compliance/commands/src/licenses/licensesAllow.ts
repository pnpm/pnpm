import { writeSettings } from '@pnpm/config.writer'
import { extractLicenseIds } from '@pnpm/deps.compliance.license-checker'
import { PnpmError } from '@pnpm/error'
import type { LicensesConfig, ProjectManifest } from '@pnpm/types'

import type { LicensesCommandResult } from './LicensesCommandResult.js'

export interface LicensesAllowOptions {
  rootProjectManifest?: ProjectManifest
  rootProjectManifestDir: string
  workspaceDir?: string
  licenses?: LicensesConfig
}

export async function licensesAllow (
  opts: LicensesAllowOptions,
  licenses: string[]
): Promise<LicensesCommandResult> {
  if (licenses.length === 0) {
    throw new PnpmError(
      'LICENSES_ALLOW_NO_ARGS',
      'Please specify at least one license to allow'
    )
  }

  if (!opts.workspaceDir) {
    throw new PnpmError(
      'LICENSES_NO_WORKSPACE',
      'Cannot modify license settings outside of a workspace'
    )
  }

  const currentAllowed = opts.licenses?.allowed ?? []
  const currentDisallowed = opts.licenses?.disallowed ?? []
  const newAllowed = [...new Set([...currentAllowed, ...licenses])]
  const newDisallowed = currentDisallowed.filter((l) => !licenses.includes(l))

  const updatedConfig: LicensesConfig = {
    ...opts.licenses,
    allowed: newAllowed,
  }
  if (newDisallowed.length !== currentDisallowed.length) {
    updatedConfig.disallowed = newDisallowed.length > 0 ? newDisallowed : undefined
  }

  await writeSettings({
    ...opts,
    workspaceDir: opts.workspaceDir,
    updatedSettings: {
      licenses: updatedConfig,
    },
  })

  const added = licenses.filter((l) => !currentAllowed.includes(l))
  const lines: string[] = []
  if (added.length > 0) {
    lines.push(`Added to allowed licenses: ${added.join(', ')}`)
  } else {
    lines.push('All specified licenses are already in the allowed list')
  }

  const removedFromDisallowed = currentDisallowed.filter((l) => licenses.includes(l))
  if (removedFromDisallowed.length > 0) {
    lines.push(`Removed from disallowed licenses: ${removedFromDisallowed.join(', ')}`)
  }

  const unrecognized = licenses.filter((l) => extractLicenseIds(l).length === 0)
  if (unrecognized.length > 0) {
    lines.push(`Note: not standard SPDX identifiers (will still be matched literally): ${unrecognized.join(', ')}`)
  }

  return { output: lines.join('\n'), exitCode: 0 }
}
