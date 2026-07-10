import { writeSettings } from '@pnpm/config.writer'
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
  const currentTarget = opts.licenses?.[target] ?? []
  const currentOther = opts.licenses?.[other] ?? []
  const newTarget = [...new Set([...currentTarget, ...ids])]
  const newOther = currentOther.filter((l) => !ids.includes(l))

  const updatedConfig: LicensesConfig = { ...opts.licenses, [target]: newTarget }
  if (newOther.length !== currentOther.length) {
    updatedConfig[other] = newOther.length > 0 ? newOther : undefined
  }

  await writeSettings({
    ...opts,
    workspaceDir,
    updatedSettings: { licenses: updatedConfig },
  })

  const added = ids.filter((l) => !currentTarget.includes(l))
  const lines: string[] = []
  lines.push(added.length > 0
    ? `Added to ${target} licenses: ${added.join(', ')}`
    : `All specified licenses are already in the ${target} list`)
  const removedFromOther = currentOther.filter((l) => ids.includes(l))
  if (removedFromOther.length > 0) {
    lines.push(`Removed from ${other} licenses: ${removedFromOther.join(', ')}`)
  }
  return { output: lines.join('\n'), exitCode: 0 }
}
