import audit, { AuditReport, AuditVulnerabilityCounts } from '@pnpm/audit'
import { docsUrl, TABLE_OPTIONS } from '@pnpm/cli-utils'
import { Config, types as allTypes, UniversalOptions } from '@pnpm/config'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { Registries } from '@pnpm/types'
import { table } from '@zkochan/table'
import chalk from 'chalk'
import pick from 'ramda/src/pick.js'
import renderHelp from 'render-help'
import fix from './fix'
import getCredentialsByURI from 'credentials-by-uri'

// eslint-disable
const AUDIT_LEVEL_NUMBER = {
  low: 0,
  moderate: 1,
  high: 2,
  critical: 3,
}

const AUDIT_COLOR = {
  low: chalk.bold,
  moderate: chalk.bold.yellow,
  high: chalk.bold.red,
  critical: chalk.bold.red,
}
// eslint-enable

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {
    ...pick([
      'dev',
      'json',
      'only',
      'optional',
      'production',
      'registry',
    ], allTypes),
    'audit-level': ['low', 'moderate', 'high', 'critical'],
    fix: Boolean,
    'ignore-registry-errors': Boolean,
  }
}

export const shorthands = {
  D: '--dev',
  P: '--production',
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
            description: 'Add overrides to the package.json file in order to force non-vulnerable versions of the dependencies',
            name: '--fix',
          },
          {
            description: 'Output audit report in JSON format',
            name: '--json',
          },
          {
            description: 'Only print advisories with severity greater than or equal to one of the following: low|moderate|high|critical. Default: low',
            name: '--audit-level <severity>',
          },
          {
            description: 'Only audit "devDependencies"',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: 'Only audit "dependencies" and "optionalDependencies"',
            name: '--prod',
            shortAlias: '-P',
          },
          {
            description: 'Don\'t audit "optionalDependencies"',
            name: '--no-optional',
          },
          {
            description: 'Use exit code 0 if the registry responds with an error. Useful when audit checks are used in CI. A build should fail because the registry has issues.',
            name: '--ignore-registry-errors',
          },
        ],
      },
    ],
    url: docsUrl('audit'),
    usages: ['pnpm audit [options]'],
  })
}

export async function handler (
  opts: Pick<UniversalOptions, 'dir'> & {
    auditLevel?: 'low' | 'moderate' | 'high' | 'critical'
    fix?: boolean
    ignoreRegistryErrors?: boolean
    json?: boolean
    lockfileDir?: string
    registries: Registries
  } & Pick<Config, 'ca'
  | 'cert'
  | 'httpProxy'
  | 'httpsProxy'
  | 'key'
  | 'localAddress'
  | 'maxSockets'
  | 'noProxy'
  | 'strictSsl'
  | 'fetchRetries'
  | 'fetchRetryMaxtimeout'
  | 'fetchRetryMintimeout'
  | 'fetchRetryFactor'
  | 'fetchTimeout'
  | 'production'
  | 'dev'
  | 'optional'
  | 'alwaysAuth'
  | 'userConfig'
  | 'rawConfig'
  >
) {
  const lockfile = await readWantedLockfile(opts.lockfileDir ?? opts.dir, { ignoreIncompatible: true })
  if (lockfile == null) {
    throw new PnpmError('AUDIT_NO_LOCKFILE', `No ${WANTED_LOCKFILE} found: Cannot audit a project without a lockfile`)
  }
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  let auditReport!: AuditReport
  const getCredentials = (registry: string) => getCredentialsByURI(opts.rawConfig, registry, opts.userConfig)
  try {
    auditReport = await audit(lockfile, getCredentials, {
      agentOptions: {
        ca: opts.ca,
        cert: opts.cert,
        httpProxy: opts.httpProxy,
        httpsProxy: opts.httpsProxy,
        key: opts.key,
        localAddress: opts.localAddress,
        maxSockets: opts.maxSockets,
        noProxy: opts.noProxy,
        strictSsl: opts.strictSsl,
        timeout: opts.fetchTimeout,
      },
      include,
      registry: opts.registries.default,
      retry: {
        factor: opts.fetchRetryFactor,
        maxTimeout: opts.fetchRetryMaxtimeout,
        minTimeout: opts.fetchRetryMintimeout,
        retries: opts.fetchRetries,
      },
      timeout: opts.fetchTimeout,
    })
  } catch (err: any) { // eslint-disable-line
    if (opts.ignoreRegistryErrors) {
      return {
        exitCode: 0,
        output: err.message,
      }
    }
  }
  if (opts.fix) {
    const newOverrides = await fix(opts.dir, auditReport)
    if (Object.values(newOverrides).length === 0) {
      return {
        exitCode: 0,
        output: 'No fixes were made',
      }
    }
    return {
      exitCode: 0,
      output: `${Object.values(newOverrides).length} overrides were added to package.json to fix vulnerabilities.
Run "pnpm install" to apply the fixes.

The added overrides:
${JSON.stringify(newOverrides, null, 2)}`,
    }
  }
  const vulnerabilities = auditReport.metadata.vulnerabilities
  const totalVulnerabilityCount = Object.values(vulnerabilities)
    .reduce((sum: number, vulnerabilitiesCount: number) => sum + vulnerabilitiesCount, 0)
  if (opts.json) {
    return {
      exitCode: totalVulnerabilityCount > 0 ? 1 : 0,
      output: JSON.stringify(auditReport, null, 2),
    }
  }

  let output = ''
  const auditLevel = AUDIT_LEVEL_NUMBER[opts.auditLevel ?? 'low']
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
  return {
    exitCode: output ? 1 : 0,
    output: `${output}${reportSummary(auditReport.metadata.vulnerabilities, totalVulnerabilityCount)}`,
  }
}

function reportSummary (vulnerabilities: AuditVulnerabilityCounts, totalVulnerabilityCount: number) {
  if (totalVulnerabilityCount === 0) return 'No known vulnerabilities found\n'
  return `${chalk.red(totalVulnerabilityCount)} vulnerabilities found\nSeverity: ${
    Object.entries(vulnerabilities)
      .filter(([auditLevel, vulnerabilitiesCount]) => vulnerabilitiesCount > 0)
      .map(([auditLevel, vulnerabilitiesCount]: [string, number]) => AUDIT_COLOR[auditLevel](`${vulnerabilitiesCount} ${auditLevel}`))
      .join(' | ')
  }`
}
