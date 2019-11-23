import audit, { AuditVulnerabilityCounts } from '@pnpm/audit'
import { docsUrl, TABLE_OPTIONS } from '@pnpm/cli-utils'
import { types as allTypes, UniversalOptions } from '@pnpm/config'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { IncludedDependencies, Registries } from '@pnpm/types'
import chalk = require('chalk')
import R = require('ramda')
import renderHelp = require('render-help')
import { table } from 'table'

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

export function types () {
  return R.pick([
    'audit-level',
    'dev',
    'json',
    'only',
    'optional',
    'production',
  ], allTypes)
}

export const commandNames = ['audit']

export function help () {
  return renderHelp({
    description: 'Checks for known security issues with the installed packages.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Output audit report in JSON format',
            name: '--json',
          },
          {
            description: 'Only print advisories with severity greater than or equal to one of the following: low|moderate|high|critical. Default: low',
            name: '--audit-level <severity>',
          },
          {
            description: 'Only audit dev dependencies',
            name: '--dev',
          },
          {
            description: 'Only audit prod dependencies',
            name: '--prod',
          },
        ],
      },
    ],
    url: docsUrl('audit'),
    usages: ['pnpm audit [options]'],
  })
}

export async function handler (
  args: string[],
  opts: Pick<UniversalOptions, 'dir'> & {
    auditLevel?: 'low' | 'moderate' | 'high' | 'critical',
    include: IncludedDependencies
    json?: boolean,
    lockfileDir?: string,
    registries: Registries,
  },
  command: string,
) {
  const lockfile = await readWantedLockfile(opts.lockfileDir || opts.dir, { ignoreIncompatible: true })
  if (!lockfile) {
    throw new PnpmError('AUDIT_NO_LOCKFILE', `No ${WANTED_LOCKFILE} found: Cannot audit a project without a lockfile`)
  }
  const auditReport = await audit(lockfile, { include: opts.include, registry: opts.registries.default })
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
