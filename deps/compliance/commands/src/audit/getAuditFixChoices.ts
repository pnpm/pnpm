import { stripVTControlCharacters } from 'node:util'

import type { AuditAdvisory, AuditLevelString } from '@pnpm/deps.compliance.audit'
import { getBorderCharacters, table } from '@zkochan/table'
import chalk from 'chalk'
import { groupBy } from 'ramda'

const AUDIT_COLOR: Record<AuditLevelString, (s: string) => string> = {
  info: chalk.dim,
  low: chalk.bold,
  moderate: chalk.bold.yellow,
  high: chalk.bold.red,
  critical: chalk.bold.red,
}

const SEVERITY_ORDER: AuditLevelString[] = ['critical', 'high', 'moderate', 'low', 'info']

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

  const grouped = groupBy((a: AuditAdvisory) => a.severity, fixable)

  const header = ['Package', 'Severity', 'Vulnerable', 'Patched', 'Title']

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
      { raw: header, key: '', disabled: true },
    ]

    for (const advisory of groupAdvisories) {
      const key = `${advisory.module_name}@${advisory.vulnerable_versions}`
      rows.push({
        raw: [
          advisory.module_name,
          AUDIT_COLOR[advisory.severity](advisory.severity),
          advisory.vulnerable_versions,
          advisory.patched_versions ?? '',
          advisory.title,
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
  return table(
    rows,
    {
      border: getBorderCharacters('void'),
      columnDefault: {
        paddingLeft: 0,
        paddingRight: 1,
        wrapWord: true,
      },
      columns: {
        0: { width: 30, truncate: 60 },
        1: { width: 10 },
        2: { width: 20, truncate: 40 },
        3: { width: 15, truncate: 30 },
        4: { width: Math.min(getColumnWidth(rows, 4, 20), 50), truncate: 100 },
      },
      drawHorizontalLine: () => false,
    }
  ).split('\n')
}

function getColumnWidth (rows: string[][], columnIndex: number, minWidth: number): number {
  return rows.reduce((max, row) => {
    if (row[columnIndex] == null) return max
    return Math.max(max, stripVTControlCharacters(row[columnIndex]).length)
  }, minWidth)
}
