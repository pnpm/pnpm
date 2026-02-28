import { audit, type AuditLevelNumber, type AuditLevelString, type AuditReport, type AuditVulnerabilityCounts, type IgnoredAuditVulnerabilityCounts } from '@pnpm/audit'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { docsUrl, TABLE_OPTIONS } from '@pnpm/cli-utils'
import { type Config, types as allTypes, type UniversalOptions } from '@pnpm/config'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { type Registries } from '@pnpm/types'
import { table } from '@zkochan/table'
import chalk, { type ChalkInstance } from 'chalk'
import { difference, pick, pickBy } from 'ramda'
import renderHelp from 'render-help'
import { fix } from './fix.js'
import { ignore } from './ignore.js'

// eslint-disable
const AUDIT_LEVEL_NUMBER = {
  low: 0,
  moderate: 1,
  high: 2,
  critical: 3,
} satisfies Record<AuditLevelString, AuditLevelNumber>

const AUDIT_COLOR = {
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
// eslint-enable

const MAX_PATHS_COUNT = 3

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
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
    ignore: [String, Array],
    'ignore-unfixable': Boolean,
  }
}

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['audit']

export function help (): string {
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
            description: 'Use exit code 0 if the registry responds with an error. Useful when audit checks are used in CI. A build should not fail because the registry has issues.',
            name: '--ignore-registry-errors',
          },
          {
            description: 'Ignore a vulnerability by CVE',
            name: '--ignore <vulnerability>',
          },
          {
            description: 'Ignore all CVEs with no resolution',
            name: '--ignore-unfixable',
          },
        ],
      },
    ],
    url: docsUrl('audit'),
    usages: ['pnpm audit [options]'],
  })
}

export type AuditOptions = Pick<UniversalOptions, 'dir'> & {
  fix?: boolean
  ignoreRegistryErrors?: boolean
  json?: boolean
  lockfileDir?: string
  registries: Registries
  ignore?: string[]
  ignoreUnfixable?: boolean
} & Pick<Config, 'auditConfig'
| 'auditLevel'
| 'ca'
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
| 'overrides'
| 'optional'
| 'userConfig'
| 'rawConfig'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
| 'virtualStoreDirMaxLength'
| 'workspaceDir'
>

export async function handler (opts: AuditOptions): Promise<{ exitCode: number, output: string }> {
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  if (lockfile == null) {
    throw new PnpmError('AUDIT_NO_LOCKFILE', `No ${WANTED_LOCKFILE} found: Cannot audit a project without a lockfile`)
  }
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  let auditReport!: AuditReport
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.rawConfig, userSettings: opts.userConfig })
  try {
    auditReport = await audit(lockfile, getAuthHeader, {
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
      lockfileDir,
      registry: opts.registries.default,
      retry: {
        factor: opts.fetchRetryFactor,
        maxTimeout: opts.fetchRetryMaxtimeout,
        minTimeout: opts.fetchRetryMintimeout,
        retries: opts.fetchRetries,
      },
      timeout: opts.fetchTimeout,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    })
  } catch (err: any) { // eslint-disable-line
    if (opts.ignoreRegistryErrors) {
      return {
        exitCode: 0,
        output: err.message,
      }
    }

    throw err
  }
  if (opts.fix) {
    const newOverrides = await fix(auditReport, opts)
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
  if (opts.ignore !== undefined || opts.ignoreUnfixable) {
    const newIgnores = await ignore({
      auditConfig: opts.auditConfig,
      auditReport,
      ignore: opts.ignore,
      ignoreUnfixable: opts.ignoreUnfixable === true,
      dir: opts.dir,
      rootProjectManifest: opts.rootProjectManifest,
      rootProjectManifestDir: opts.rootProjectManifestDir,
      workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
    })
    if (newIgnores.length === 0) {
      return {
        exitCode: 0,
        output: 'No new vulnerabilities were ignored',
      }
    }
    return {
      exitCode: 0,
      output: `${newIgnores.length} new vulnerabilities were ignored:
${newIgnores.join('\n')}`,
    }
  }
  const vulnerabilities = auditReport.metadata.vulnerabilities
  const ignoredVulnerabilities: IgnoredAuditVulnerabilityCounts = {
    low: 0,
    moderate: 0,
    high: 0,
    critical: 0,
  }
  const totalVulnerabilityCount = Object.values(vulnerabilities)
    .reduce((sum: number, vulnerabilitiesCount: number) => sum + vulnerabilitiesCount, 0)
  const ignoreGhsas = opts.auditConfig?.ignoreGhsas
  if (ignoreGhsas) {
    auditReport.advisories = pickBy(({ github_advisory_id: githubAdvisoryId, severity, findings }) => {
      if (!ignoreGhsas.includes(githubAdvisoryId)) {
        return true
      }

      const occurrences = (findings || []).reduce((acc: number, finding: { paths?: string[] }) => acc + (finding.paths?.length || 0), 0)
      ignoredVulnerabilities[severity as AuditLevelString] += occurrences

      return false
    }, auditReport.advisories)
  }
  const ignoreCves = opts.auditConfig?.ignoreCves
  if (ignoreCves) {
    auditReport.advisories = pickBy(({ cves, severity, findings }) => {
      if (cves.length === 0 || difference(cves, ignoreCves).length > 0) {
        return true
      }

      const occurrences = (findings || []).reduce((acc: number, finding: { paths?: string[] }) => acc + (finding.paths?.length || 0), 0)
      ignoredVulnerabilities[severity as AuditLevelString] += occurrences

      return false
    }, auditReport.advisories)
  }
  const auditLevel = AUDIT_LEVEL_NUMBER[opts.auditLevel ?? 'low']
  const advisoryEntries = Object.entries(auditReport.advisories)
    .filter(([, { severity }]) => AUDIT_LEVEL_NUMBER[severity] >= auditLevel)
  if (opts.json) {
    const advisories = Object.fromEntries(advisoryEntries)
    return {
      exitCode: Object.keys(advisories).length > 0 ? 1 : 0,
      output: JSON.stringify({ ...auditReport, advisories }, null, 2),
    }
  }

  let output = ''
  advisoryEntries.sort(([, a1], [, a2]) => AUDIT_LEVEL_NUMBER[a2.severity] - AUDIT_LEVEL_NUMBER[a1.severity])
  for (const [, advisory] of advisoryEntries) {
    const paths = advisory.findings.map(({ paths }) => paths).flat()
    output += table([
      [AUDIT_COLOR[advisory.severity](advisory.severity), chalk.bold(advisory.title)],
      ['Package', advisory.module_name],
      ['Vulnerable versions', advisory.vulnerable_versions],
      ['Patched versions', advisory.patched_versions],
      [
        'Paths',
        (paths.length > MAX_PATHS_COUNT
          ? paths
            .slice(0, MAX_PATHS_COUNT)
            .concat([
              `... Found ${paths.length} paths, run \`pnpm why ${advisory.module_name}\` for more information`,
            ])
          : paths
        ).join('\n\n'),
      ],
      ['More info', advisory.url],
    ], AUDIT_TABLE_OPTIONS)
  }
  return {
    exitCode: output ? 1 : 0,
    output: `${output}${reportSummary(auditReport.metadata.vulnerabilities, totalVulnerabilityCount, ignoredVulnerabilities)}`,
  }
}

function reportSummary (vulnerabilities: AuditVulnerabilityCounts, totalVulnerabilityCount: number, ignoredVulnerabilities: IgnoredAuditVulnerabilityCounts): string {
  if (totalVulnerabilityCount === 0) return 'No known vulnerabilities found\n'
  return `${chalk.red(totalVulnerabilityCount)} vulnerabilities found\nSeverity: ${
    Object.entries(vulnerabilities)
      .filter(([_auditLevel, vulnerabilitiesCount]) => vulnerabilitiesCount > 0)
      .map(([auditLevel, vulnerabilitiesCount]) => AUDIT_COLOR[auditLevel as AuditLevelString](`${vulnerabilitiesCount as string} ${auditLevel}${ignoredVulnerabilities[auditLevel as AuditLevelString] > 0 ? ` (${ignoredVulnerabilities[auditLevel as AuditLevelString]} ignored)` : ''}`))
      .join(' | ')
  }`
}
