import { writeSettings } from '@pnpm/config.writer'
import { assertNoCompoundPolicyEntries } from '@pnpm/deps.compliance.license-checker'
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
  const ids = [...new Set(licenses.map((license) => license.trim()).filter((license) => license.length > 0))]
  assertNoCompoundPolicyEntries({ [target]: ids })

  // Case-insensitive throughout: the matcher treats "mit" and "MIT" as the same
  // license, so allowing "mit" must not add a case-variant duplicate alongside an
  // existing "MIT" in the same list, and must prune "MIT" from the opposite list.
  const currentTarget = opts.licenses?.[target] ?? []
  const currentOther = opts.licenses?.[other] ?? []
  const added = findLicensesNotInList(ids, currentTarget)
  const removedFromOther = currentOther.filter((license) => includesCaseInsensitive(ids, license))

  const updatedConfig: LicensesConfig = { ...opts.licenses, [target]: [...currentTarget, ...added] }
  if (removedFromOther.length > 0) {
    const newOther = currentOther.filter((license) => !includesCaseInsensitive(ids, license))
    updatedConfig[other] = newOther.length > 0 ? newOther : undefined
  }

  await writeSettings({
    ...opts,
    workspaceDir,
    updatedSettings: { licenses: updatedConfig },
  })

  const lines = [
    added.length > 0
      ? `Added to ${target} licenses: ${added.join(', ')}`
      : `All specified licenses are already in the ${target} list`,
  ]
  if (removedFromOther.length > 0) {
    lines.push(`Removed from ${other} licenses: ${removedFromOther.join(', ')}`)
  }
  return { output: lines.join('\n'), exitCode: 0 }
}

function findLicensesNotInList (ids: string[], list: string[]): string[] {
  const seen = new Set(list.map((license) => license.toLowerCase()))
  const missing: string[] = []
  for (const id of ids) {
    const lower = id.toLowerCase()
    if (seen.has(lower)) continue
    seen.add(lower)
    missing.push(id)
  }
  return missing
}

function includesCaseInsensitive (list: string[], value: string): boolean {
  const lower = value.toLowerCase()
  return list.some((entry) => entry.toLowerCase() === lower)
}
