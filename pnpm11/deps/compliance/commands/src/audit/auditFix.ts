import { writeSettings } from '@pnpm/config.writer'
import { type AuditReport, normalizeGhsaId } from '@pnpm/deps.compliance.audit'
import { globalInfo } from '@pnpm/logger'
import { getRangeSpecStyle } from '@pnpm/pkg-manifest.utils'
import { sanitizeInline } from '@pnpm/text.sanitize'

import type { AuditOptions } from './audit.js'
import { AUDIT_LEVEL_NUMBER, formatFixWithUpdateOutput } from './auditOutput.js'
import { fix } from './fix.js'
import { fixWithUpdate, type FixWithUpdateOptions } from './fixWithUpdate.js'
import { interactiveAuditFix } from './interactiveAuditFix.js'
import { pruneIgnoredGhsas } from './pruneIgnoredGhsas.js'
import type { PublishTimesFetcher } from './publishTimes.js'

export type FixMethod = 'update' | 'override'

export interface AuditFixContext {
  fixMethod: FixMethod
  getPublishTimes: PublishTimesFetcher
  include: FixWithUpdateOptions['include']
}

export interface AuditCommandResult {
  exitCode: number
  output: string
}

export async function runAuditFix (auditReport: AuditReport, opts: AuditOptions, context: AuditFixContext): Promise<AuditCommandResult> {
  if (opts.auditIgnorePrune && opts.auditConfig?.ignoreGhsas?.length) {
    await pruneUnusedIgnoredGhsas(opts.auditConfig.ignoreGhsas, auditReport, opts)
  }
  // Pre-filter by auditLevel and ignoreGhsas so the interactive prompt
  // and the update-method path see the same set of advisories that
  // fix.ts's getFixableAdvisories filters for the override path.
  let filteredAuditReport: AuditReport = {
    ...auditReport,
    advisories: filterAdvisoriesForFix(auditReport.advisories, opts),
  }
  if (opts.interactive) {
    filteredAuditReport = await interactiveAuditFix(filteredAuditReport, getRangeSpecStyle(opts))
  }
  if (context.fixMethod === 'update') {
    return fixVulnerabilitiesWithUpdate(filteredAuditReport, { ...opts, getPublishTimes: context.getPublishTimes, include: context.include })
  }
  return fixVulnerabilitiesWithOverrides(filteredAuditReport, { ...opts, getPublishTimes: context.getPublishTimes })
}

async function pruneUnusedIgnoredGhsas (configuredGhsas: string[], auditReport: AuditReport, opts: AuditOptions): Promise<void> {
  const { pruned, retained } = pruneIgnoredGhsas(configuredGhsas, auditReport)
  if (pruned.length > 0) {
    // The pruned ids keep their original spelling from the
    // repository-controlled workspace manifest, so strip control
    // characters before they reach the terminal.
    globalInfo(`Removed ${pruned.length} unused ignored GHSA${pruned.length === 1 ? '' : 's'}: ${pruned.map(sanitizeInline).join(', ')}`)
  }
  // Persist even when nothing was removed: `retained` may still differ
  // from the configured list (deduplicated or case-normalized), and the
  // file should always reflect the canonical form.
  const retainedDiffers = retained.length !== configuredGhsas.length ||
    retained.some((ghsa, index) => ghsa !== configuredGhsas[index])
  if (!retainedDiffers) return
  // Written through the dedicated ignore-list update so the retained
  // list lands on whichever spelling the manifest uses — replacing
  // only `auditConfig` would let a canonical `audit.ignore` list
  // shadow the pruned result on the next read.
  await writeSettings({
    ...opts,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
    updatedAuditIgnoreGhsas: retained,
  })
  // Update opts for subsequent operations
  opts.auditConfig = {
    ...opts.auditConfig,
    ignoreGhsas: retained.length > 0 ? retained : undefined,
  }
}

function filterAdvisoriesForFix (
  advisories: AuditReport['advisories'],
  opts: Pick<AuditOptions, 'auditLevel' | 'auditConfig'>
): AuditReport['advisories'] {
  const auditLevel = AUDIT_LEVEL_NUMBER[opts.auditLevel ?? 'low']
  const ignoreGhsas = opts.auditConfig?.ignoreGhsas
  const ignoreGhsaSet = ignoreGhsas?.length ? new Set(ignoreGhsas.map(normalizeGhsaId)) : undefined
  return Object.fromEntries(
    Object.entries(advisories).filter(([, { severity, github_advisory_id: ghsaId }]) => {
      if (AUDIT_LEVEL_NUMBER[severity] < auditLevel) return false
      if (ignoreGhsaSet && ghsaId && ignoreGhsaSet.has(normalizeGhsaId(ghsaId))) return false
      return true
    })
  )
}

async function fixVulnerabilitiesWithUpdate (auditReport: AuditReport, opts: FixWithUpdateOptions): Promise<AuditCommandResult> {
  const result = await fixWithUpdate(auditReport, opts)
  let output = formatFixWithUpdateOutput(result, auditReport)
  if (result.addedAgeExcludes.length > 0) {
    output += `\n${result.addedAgeExcludes.length} entries were added to minimumReleaseAgeExclude to allow installing the patched versions:\n${result.addedAgeExcludes.join('\n')}\n`
  }
  return {
    exitCode: result.remaining.length > 0 ? 1 : 0,
    output,
  }
}

async function fixVulnerabilitiesWithOverrides (auditReport: AuditReport, opts: AuditOptions): Promise<AuditCommandResult> {
  const { vulnOverrides, addedAgeExcludes } = await fix(auditReport, opts)
  if (Object.values(vulnOverrides).length === 0) {
    return {
      exitCode: 0,
      output: 'No fixes were made',
    }
  }
  let output = `${Object.values(vulnOverrides).length} overrides were added to pnpm-workspace.yaml to fix vulnerabilities.
Run "pnpm install" to apply the fixes.

The added overrides:
${JSON.stringify(vulnOverrides, null, 2)}`
  if (addedAgeExcludes.length > 0) {
    output += `\n\n${addedAgeExcludes.length} entries were added to minimumReleaseAgeExclude to allow installing the patched versions:\n${addedAgeExcludes.join('\n')}`
  }
  return {
    exitCode: 0,
    output,
  }
}
