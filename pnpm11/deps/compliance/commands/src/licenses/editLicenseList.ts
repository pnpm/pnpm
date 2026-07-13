import { writeSettings } from '@pnpm/config.writer'
import { isCompoundLicenseExpression } from '@pnpm/deps.compliance.license-checker'
import { PnpmError } from '@pnpm/error'
import type { LicensesConfig, ProjectManifest } from '@pnpm/types'

import type { LicensesCommandResult } from './LicensesCommandResult.js'

export interface EditLicenseListOptions {
  rootProjectManifest?: ProjectManifest
  rootProjectManifestDir: string
  workspaceDir?: string
  licenses?: LicensesConfig
}

const NO_ARGS: Record<'allowed' | 'disallowed', string> = {
  allowed: 'LICENSES_ALLOW_NO_ARGS',
  disallowed: 'LICENSES_DISALLOW_NO_ARGS',
}

export async function editLicenseList (
  opts: EditLicenseListOptions,
  licenses: string[],
  target: 'allowed' | 'disallowed'
): Promise<LicensesCommandResult> {
  if (licenses.length === 0) {
    throw new PnpmError(NO_ARGS[target], `Please specify at least one license to ${target === 'allowed' ? 'allow' : 'disallow'}`)
  }

  const workspaceDir = opts.workspaceDir ?? opts.rootProjectManifestDir
  if (!workspaceDir) {
    throw new PnpmError('LICENSES_NO_WORKSPACE', 'Cannot modify license settings outside of a project')
  }

  const other: 'allowed' | 'disallowed' = target === 'allowed' ? 'disallowed' : 'allowed'
  const ids = [...new Set(licenses.map((l) => l.trim()).filter((l) => l.length > 0))]

  const compound = ids.filter(isCompoundLicenseExpression)
  if (compound.length > 0) {
    throw new PnpmError(
      'LICENSES_COMPOUND_EXPRESSION',
      `Compound license expressions (AND/OR) are not supported in the allowed/disallowed list: ${compound.join(', ')}. ` +
      'List each license identifier separately, e.g. "pnpm licenses allow MIT Apache-2.0".'
    )
  }

  const currentTarget = opts.licenses?.[target] ?? []
  const currentOther = opts.licenses?.[other] ?? []
  // Case-insensitive throughout: the matcher treats "mit" and "MIT" as the same
  // license, so allowing "mit" must not add a case-variant duplicate alongside an
  // existing "MIT" in the same list, and must prune "MIT" from the opposite list.
  const currentTargetLower = new Set(currentTarget.map((l) => l.toLowerCase()))
  const added: string[] = []
  const addedLower = new Set<string>()
  for (const id of ids) {
    const lower = id.toLowerCase()
    if (currentTargetLower.has(lower) || addedLower.has(lower)) continue
    addedLower.add(lower)
    added.push(id)
  }
  const newTarget = [...currentTarget, ...added]
  const newOther = currentOther.filter((l) => !ids.some((id) => id.toLowerCase() === l.toLowerCase()))

  const updatedConfig: LicensesConfig = { ...opts.licenses, [target]: newTarget }
  if (newOther.length !== currentOther.length) {
    updatedConfig[other] = newOther.length > 0 ? newOther : undefined
  }

  await writeSettings({
    ...opts,
    workspaceDir,
    updatedSettings: { licenses: updatedConfig },
  })

  const lines: string[] = []
  lines.push(added.length > 0
    ? `Added to ${target} licenses: ${added.join(', ')}`
    : `All specified licenses are already in the ${target} list`)
  const removedFromOther = currentOther.filter((l) => ids.some((id) => id.toLowerCase() === l.toLowerCase()))
  if (removedFromOther.length > 0) {
    lines.push(`Removed from ${other} licenses: ${removedFromOther.join(', ')}`)
  }
  return { output: lines.join('\n'), exitCode: 0 }
}
