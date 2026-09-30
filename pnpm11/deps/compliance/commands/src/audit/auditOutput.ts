import { TABLE_OPTIONS } from '@pnpm/cli.utils'
import type { AuditAdvisory, AuditLevelNumber, AuditLevelString, AuditReport, AuditVulnerabilityCounts, IgnoredAuditVulnerabilityCounts } from '@pnpm/deps.compliance.audit'
import { table } from '@zkochan/table'
import chalk, { type ChalkInstance } from 'chalk'

import type { FixWithUpdateResult } from './fixWithUpdate.js'

export const AUDIT_LEVEL_NUMBER = {
  info: 0,
  low: 1,
  moderate: 2,
  high: 3,
  critical: 4,
} satisfies Record<AuditLevelString, AuditLevelNumber>

const AUDIT_COLOR = {
  info: chalk.dim,
  low: chalk.bold,
  moderate: chalk.bold.yellow,
  high: chalk.bold.red,
  critical: chalk.bold.red,
} satisfies Record<AuditLevelString, ChalkInstance>

const AUDIT_TABLE_OPTIONS = {
  ...TABLE_OPTIONS,
  columns: {
    1: {
      width: 54, // = table width of 80
      wrapWord: true,
    },
  },
}

const MAX_PATHS_COUNT = 3

/**
 * Renders one table per advisory, most severe first. Sorts `advisoryEntries`
 * in place.
 */
export function renderAdvisoryTables (advisoryEntries: Array<[string, AuditAdvisory]>): string {
  let output = ''
  advisoryEntries.sort(([, a1], [, a2]) => AUDIT_LEVEL_NUMBER[a2.severity] - AUDIT_LEVEL_NUMBER[a1.severity])
  for (const [, advisory] of advisoryEntries) {
    output += renderAdvisoryTable(advisory)
  }
  return output
}

function renderAdvisoryTable (advisory: AuditAdvisory): string {
  return table([
    [AUDIT_COLOR[advisory.severity](advisory.severity), chalk.bold(advisory.title)],
    ['Package', advisory.module_name],
    ['Vulnerable versions', advisory.vulnerable_versions],
    ['Patched versions', advisory.patched_versions ?? (advisory.patched_versions_unpublished === true ? 'None' : '(unknown)')],
    ['Paths', formatAdvisoryPaths(advisory)],
    ['More info', advisory.url],
  ], AUDIT_TABLE_OPTIONS)
}

function formatAdvisoryPaths (advisory: AuditAdvisory): string {
  const paths = advisory.findings.map(({ paths }) => paths).flat()
  if (paths.length <= MAX_PATHS_COUNT) return paths.join('\n\n')
  return paths
    .slice(0, MAX_PATHS_COUNT)
    .concat([
      `... Found ${paths.length} paths, run \`pnpm why ${advisory.module_name}\` for more information`,
    ])
    .join('\n\n')
}

export function reportSummary (vulnerabilities: AuditVulnerabilityCounts, ignoredVulnerabilities: IgnoredAuditVulnerabilityCounts): string {
  const auditLevels = Object.keys(vulnerabilities) as AuditLevelString[]
  const found = auditLevels.map((auditLevel) => ({ auditLevel, count: vulnerabilities[auditLevel] }))
  const ignored = auditLevels.map((auditLevel) => ({
    auditLevel,
    count: ignoredVulnerabilities[auditLevel],
  }))
  const totalIgnoredCount = sumSeverityCounts(ignored)
  const ignoredSummary = totalIgnoredCount === 0 ? '' : `\n${totalIgnoredCount} ignored: ${listSeverityCounts(ignored)}`
  const totalVulnerabilityCount = sumSeverityCounts(found)
  if (totalVulnerabilityCount === 0) {
    const headline = totalIgnoredCount === 0
      ? 'No known vulnerabilities found'
      : 'All found vulnerabilities were already reviewed and decided to be ignored'
    return `${headline}${ignoredSummary}\n`
  }
  return `${chalk.red(totalVulnerabilityCount)} vulnerabilities found\nSeverity: ${listSeverityCounts(found)}${ignoredSummary}`
}

function sumSeverityCounts (severities: Array<{ count: number }>): number {
  return severities.reduce((sum, { count }) => sum + count, 0)
}

function listSeverityCounts (severities: Array<{ auditLevel: AuditLevelString, count: number }>): string {
  return severities
    .filter(({ count }) => count > 0)
    .map(({ auditLevel, count }) => AUDIT_COLOR[auditLevel](`${count} ${auditLevel}`))
    .join(' | ')
}

interface IdAndAdvisory {
  id: number
  advisory?: AuditAdvisory
}

export function formatFixWithUpdateOutput (result: FixWithUpdateResult, auditReport: AuditReport): string {
  const output: string[] = []
  const fixed = sortBySeverity(result.fixed, auditReport)
  const remaining = sortBySeverity(result.remaining, auditReport)

  const fixedString = fixed.length === 1 ? 'vulnerability was fixed' : 'vulnerabilities were fixed'
  const remainingString = remaining.length === 1 ? 'vulnerability remains' : 'vulnerabilities remain'

  output.push(`${chalk.green(fixed.length)} ${fixedString}, ${chalk.red(remaining.length)} ${remainingString}.`)

  if (fixed.length > 0) {
    output.push('\nThe fixed vulnerabilities are:')
    output.push(...fixed.map((fixedAdvisory) => summarizeAdvisory(true, fixedAdvisory)))
  }

  if (remaining.length > 0) {
    output.push('\nThe remaining vulnerabilities are:')
    output.push(...remaining.map((remainingAdvisory) => summarizeAdvisory(false, remainingAdvisory)))
  }

  // Add trailing newline
  output.push('')
  return output.join('\n')
}

/**
 * Sort the given array of advisory IDs by severity descending
 */
function sortBySeverity (ids: number[], auditReport: AuditReport): IdAndAdvisory[] {
  return ids.map(id => ({ id, advisory: auditReport.advisories[id] })).sort((left, right) => {
    const leftValue = left.advisory ? AUDIT_LEVEL_NUMBER[left.advisory.severity] : -1
    const rightValue = right.advisory ? AUDIT_LEVEL_NUMBER[right.advisory.severity] : -1
    return rightValue - leftValue
  })
}

function summarizeAdvisory (fixed: boolean, { id, advisory }: IdAndAdvisory): string {
  if (advisory) {
    const color = fixed ? chalk.green : AUDIT_COLOR[advisory.severity]
    return `- (${color(advisory.severity)}) "${color(advisory.title)}" ${chalk.blue(advisory.module_name)}`
  }
  return `- Advisory with ID ${id} (details not found in the audit report)`
}
