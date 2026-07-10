import { type LicenseViolation, sanitizeForTerminal } from '@pnpm/deps.compliance.license-checker'
import chalk from 'chalk'

import type { LicensesCommandResult } from './LicensesCommandResult.js'

export function renderCheckTable (
  violations: LicenseViolation[],
  warnings: LicenseViolation[],
  checkedCount: number
): LicensesCommandResult {
  const lines: string[] = []
  if (violations.length > 0) {
    lines.push(chalk.red(`${violations.length} license violation(s) found:`))
    lines.push('')
    for (const v of violations) {
      lines.push(`  ${chalk.red('x')} ${sanitizeForTerminal(v.packageName)}@${v.packageVersion} - ${sanitizeForTerminal(v.license)} - ${sanitizeForTerminal(v.reason)}`)
    }
  }
  if (warnings.length > 0) {
    if (lines.length > 0) lines.push('')
    lines.push(chalk.yellow(`${warnings.length} license warning(s):`))
    lines.push('')
    for (const w of warnings) {
      lines.push(`  ${chalk.yellow('!')} ${sanitizeForTerminal(w.packageName)}@${w.packageVersion} - ${sanitizeForTerminal(w.license)} - ${sanitizeForTerminal(w.reason)}`)
    }
  }
  lines.push('')
  lines.push(`Checked ${checkedCount} ${checkedCount === 1 ? 'package' : 'packages'}`)
  return { output: lines.join('\n'), exitCode: violations.length > 0 ? 1 : 0 }
}
