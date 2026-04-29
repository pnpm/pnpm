import type { AuditAdvisory, AuditLevelString } from '@pnpm/deps.compliance.audit'
import { getBorderCharacters, table } from '@zkochan/table'
import chalk from 'chalk'
import { groupBy } from 'ramda'

import { caretRangeForPatched } from './fix.js'

const AUDIT_COLOR: Record<AuditLevelString, (s: string) => string> = {
  info: chalk.dim,
  low: chalk.bold,
  moderate: chalk.bold.yellow,
  high: chalk.bold.red,
  critical: chalk.bold.red,
}

const SEVERITY_ORDER: AuditLevelString[] = ['critical', 'high', 'moderate', 'low', 'info']

const SEVERITY_RANK: Record<AuditLevelString, number> = {
  info: 0,
  low: 1,
  moderate: 2,
  high: 3,
  critical: 4,
}

const COLUMN_HEADER = ['Package', 'Vulnerable', 'Patched', 'Advisories']

export interface AuditChoiceRow {
  name: string
  value: string
  disabled?: boolean
}

type AuditChoiceGroup = Array<{
  name: string
  message: string
  choices: AuditChoiceRow[]
  disabled?: boolean
}>

export function getAuditFixChoices (advisories: AuditAdvisory[]): AuditChoiceGroup {
  if (advisories.length === 0) {
    return []
  }

  const fixable = advisories.filter(({ patched_versions: p }) => p != null)
  if (fixable.length === 0) {
    return []
  }

  // Collapse advisories that share module_name@vulnerable_versions: they
  // produce the same override, so showing them as separate rows would
  // duplicate every choice. Titles are joined; the highest severity wins.
  const deduped = dedupeByFixKey(fixable)

  const grouped = groupBy((a: AuditAdvisory) => a.severity, deduped)

  const finalChoices: AuditChoiceGroup = []
  for (const severity of SEVERITY_ORDER) {
    const groupAdvisories = grouped[severity]
    if (!groupAdvisories?.length) continue

    interface RawRow {
      raw: string[]
      key: string
      disabled?: boolean
    }

    const rows: RawRow[] = [
      { raw: COLUMN_HEADER, key: '', disabled: true },
    ]

    for (const advisory of groupAdvisories) {
      const key = `${advisory.module_name}@${advisory.vulnerable_versions}`
      rows.push({
        raw: [
          advisory.module_name,
          advisory.vulnerable_versions,
          advisory.patched_versions ? caretRangeForPatched(advisory.patched_versions) : '',
          advisory.github_advisory_id ?? '',
        ],
        key,
      })
    }

    const rendered = alignColumns(rows.map(r => r.raw))

    const choices = rows.map((row, i) => {
      if (i === 0) {
        return {
          name: rendered[i],
          value: '',
          disabled: true,
          hint: '',
        }
      }
      return {
        name: row.key,
        message: rendered[i],
        value: row.key,
      }
    })

    finalChoices.push({
      name: `[${severity}]`,
      choices,
      message: AUDIT_COLOR[severity as AuditLevelString](severity),
    })
  }

  return finalChoices
}

function alignColumns (rows: string[][]): string[] {
  // No per-column width / truncate: each cell is short (package name,
  // semver range, one or two GHSA IDs), so the table library can
  // auto-size columns and still keep each row on a single line. Keeping
  // rows 1:1 with rendered lines is required because the caller picks
  // `rendered[i]` for choice `i`.
  return table(
    rows,
    {
      border: getBorderCharacters('void'),
      columnDefault: {
        paddingLeft: 0,
        paddingRight: 2,
      },
      drawHorizontalLine: () => false,
    }
  ).split('\n').filter((line) => line.trim() !== '')
}

function dedupeByFixKey (advisories: AuditAdvisory[]): AuditAdvisory[] {
  const byKey = new Map<string, AuditAdvisory>()
  for (const advisory of advisories) {
    const key = `${advisory.module_name}@${advisory.vulnerable_versions}`
    const existing = byKey.get(key)
    if (!existing) {
      byKey.set(key, advisory)
      continue
    }
    const keepSeverity = SEVERITY_RANK[advisory.severity] > SEVERITY_RANK[existing.severity]
      ? advisory.severity
      : existing.severity
    const mergedId = existing.github_advisory_id && advisory.github_advisory_id && existing.github_advisory_id !== advisory.github_advisory_id
      ? `${existing.github_advisory_id}, ${advisory.github_advisory_id}`
      : existing.github_advisory_id || advisory.github_advisory_id
    byKey.set(key, {
      ...existing,
      severity: keepSeverity,
      github_advisory_id: mergedId,
    })
  }
  return Array.from(byKey.values())
}

