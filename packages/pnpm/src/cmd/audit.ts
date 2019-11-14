import audit, { AuditVulnerabilityCounts } from '@pnpm/audit'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import chalk = require('chalk')
import { table } from 'table'
import { TABLE_OPTIONS } from '../style'
import { PnpmOptions } from '../types'

// tslint:disable
const AUDIT_LEVEL_NUMBER = {
  'low': 0,
  'moderate': 1,
  'high': 2,
  'critical': 3,
}

const AUDIT_COLOR = {
  'low': chalk.bold,
  'moderate': chalk.bold.yellow,
  'high': chalk.bold.red,
  'critical': chalk.bold.red,
}
// tslint:enable

export default async function (
  args: string[],
  opts: PnpmOptions & {
    json?: boolean,
  },
  command: string,
) {
  const lockfile = await readWantedLockfile(opts.lockfileDir || opts.dir, { ignoreIncompatible: true })
  if (!lockfile) {
    throw new PnpmError('AUDIT_NO_LOCKFILE', `No ${WANTED_LOCKFILE} found: Cannot audit a project without a lockfile`)
  }
  const auditReport = await audit(lockfile, { registry: opts.registries.default })
  if (opts.json) {
    return JSON.stringify(auditReport, null, 2)
  }

  let output = ''
  const auditLevel = AUDIT_LEVEL_NUMBER[opts.auditLevel || 'low']
  const advisories = Object.values(auditReport.advisories)
    .filter(({ severity }) => AUDIT_LEVEL_NUMBER[severity] >= auditLevel)
    .sort((a1, a2) => AUDIT_LEVEL_NUMBER[a2.severity] - AUDIT_LEVEL_NUMBER[a1.severity])
  for (const advisory of advisories) {
    output += table([
      [AUDIT_COLOR[advisory.severity](advisory.severity), chalk.bold(advisory.title)],
      ['Package', advisory.module_name],
      ['Vulnerable versions', advisory.vulnerable_versions],
      ['Patched versions', advisory.patched_versions],
      ['More info', advisory.url],
    ], TABLE_OPTIONS)
  }
  return `${output}${reportSummary(auditReport.metadata.vulnerabilities)}`
}

function reportSummary (vulnerabilities: AuditVulnerabilityCounts) {
  const totalVulnerabilityCount = Object.values(vulnerabilities).reduce((sum, vulnerabilitiesCount) => sum + vulnerabilitiesCount, 0)
  if (totalVulnerabilityCount === 0) return 'No known vulnerabilities found'
  return `${chalk.red(totalVulnerabilityCount)} vulnerabilities found\nSeverity: ${
    Object.entries(vulnerabilities)
      .filter(([auditLevel, vulnerabilitiesCount]) => vulnerabilitiesCount > 0)
      .map(([auditLevel, vulnerabilitiesCount]) => AUDIT_COLOR[auditLevel](`${vulnerabilitiesCount} ${auditLevel}`))
      .join(' | ')
  }`
}
