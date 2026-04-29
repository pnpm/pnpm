import { docsUrl, TABLE_OPTIONS } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes, type UniversalOptions } from '@pnpm/config.reader'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { audit, type AuditAdvisory, type AuditLevelNumber, type AuditLevelString, type AuditReport, type AuditVulnerabilityCounts, type IgnoredAuditVulnerabilityCounts, normalizeGhsaId } from '@pnpm/deps.compliance.audit'
import { PnpmError } from '@pnpm/error'
import { type InstallCommandOptions, update } from '@pnpm/installing.commands'
import { readEnvLockfile, readWantedLockfile } from '@pnpm/lockfile.fs'
import { globalInfo } from '@pnpm/logger'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { Registries } from '@pnpm/types'
import { table } from '@zkochan/table'
import chalk, { type ChalkInstance } from 'chalk'
import enquirer from 'enquirer'
import { pick, pickBy } from 'ramda'
import { renderHelp } from 'render-help'

import { fix } from './fix.js'
import { fixWithUpdate, type FixWithUpdateResult } from './fixWithUpdate.js'
import { type AuditChoiceRow, getAuditFixChoices } from './getAuditFixChoices.js'
import { ignore } from './ignore.js'

const AUDIT_LEVEL_NUMBER = {
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

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...update.rcOptionsTypes(),
    ...pick([
      'dev',
      'json',
      'only',
      'optional',
      'production',
      'registry',
    ], allTypes),
    'audit-level': ['info', 'low', 'moderate', 'high', 'critical'],
    // For fix, use String instead of a list of allowed string values.
    // Otherwise, an unexpected value will get coerced to true because of the Boolean type.
    fix: [String, Boolean],
    'ignore-registry-errors': Boolean,
    ignore: [String, Array],
    'ignore-unfixable': Boolean,
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'recursive',
      'workspace',
    ], update.cliOptionsTypes()),
    ...rcOptionsTypes(),
    interactive: Boolean,
  }
}

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['audit']

export const recursiveByDefault = true

export function help (): string {
  return renderHelp({
    description: 'Checks for known security issues with the installed packages.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Fix the audited vulnerabilities using the specified method: "override" or "update". "override" adds overrides to the package.json file in order to force non-vulnerable versions of the dependencies. "update" attempts to update the vulnerable packages in the lockfile to non-vulnerable versions. If no method is specified, "override" is used by default.',
            name: '--fix [method]',
          },
          {
            description: 'Output audit report in JSON format',
            name: '--json',
          },
          {
            description: 'Only print advisories with severity greater than or equal to one of the following: info|low|moderate|high|critical. Default: low',
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
            description: 'Ignore a vulnerability by its GitHub advisory ID (e.g. GHSA-xxxx-xxxx-xxxx)',
            name: '--ignore <vulnerability>',
          },
          {
            description: 'Ignore all vulnerabilities for which no fix exists',
            name: '--ignore-unfixable',
          },
          {
            description: 'Show vulnerabilities and select which ones to fix interactively',
            name: '--interactive',
            shortAlias: '-i',
          },
        ],
      },
    ],
    url: docsUrl('audit'),
    usages: ['pnpm audit [options]'],
  })
}

export type AuditOptions = Pick<UniversalOptions, 'dir'> & {
  fix?: boolean | 'override' | 'update'
  ignoreRegistryErrors?: boolean
  interactive?: boolean
  json?: boolean
  lockfileDir?: string
  registries: Registries
  ignore?: string[]
  ignoreUnfixable?: boolean
} & Pick<Config, 'auditConfig'
| 'auditLevel'
| 'minimumReleaseAge'
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
| 'configByUri'
| 'virtualStoreDirMaxLength'
| 'workspaceDir'
> & Pick<ConfigContext,
| 'rootProjectManifest'
| 'rootProjectManifestDir'
> & InstallCommandOptions

const DEFAULT_FIX_METHOD = 'override'

export async function handler (opts: AuditOptions): Promise<{ exitCode: number, output: string }> {
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  if (lockfile == null) {
    throw new PnpmError('AUDIT_NO_LOCKFILE', `No ${WANTED_LOCKFILE} found: Cannot audit a project without a lockfile`)
  }
  const envLockfile = await readEnvLockfile(opts.workspaceDir ?? lockfileDir)
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  let auditReport!: AuditReport
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri, opts.registries?.default)
  try {
    auditReport = await audit(lockfile, getAuthHeader, {
      dispatcherOptions: {
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
      envLockfile,
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

    throw err
  }
  let fixMethod: 'update' | 'override' | undefined
  if (opts.fix === 'update' || opts.fix === 'override') {
    fixMethod = opts.fix
  } else if (opts.fix === true || (opts.interactive && !opts.fix)) {
    fixMethod = DEFAULT_FIX_METHOD
  } else if (!opts.fix) {
    fixMethod = undefined
  } else {
    throw new PnpmError('INVALID_FIX_OPTION', `Invalid value for --fix: ${opts.fix as string}. Should be one of "override" or "update"`)
  }
  if (fixMethod != null) {
    // Pre-filter by auditLevel and ignoreGhsas so the interactive prompt
    // and the update-method path see the same set of advisories that
    // fix.ts's getFixableAdvisories filters for the override path.
    let filteredAuditReport: AuditReport = {
      ...auditReport,
      advisories: filterAdvisoriesForFix(auditReport.advisories, opts),
    }
    if (opts.interactive) {
      filteredAuditReport = await interactiveAuditFix(filteredAuditReport)
    }
    if (fixMethod === 'update') {
      const result = await fixWithUpdate(filteredAuditReport, { ...opts, include })
      let output = formatFixWithUpdateOutput(result, filteredAuditReport)
      if (result.addedAgeExcludes.length > 0) {
        output += `\n${result.addedAgeExcludes.length} entries were added to minimumReleaseAgeExclude to allow installing the patched versions:\n${result.addedAgeExcludes.join('\n')}\n`
      }
      return {
        exitCode: result.remaining.length > 0 ? 1 : 0,
        output,
      }
    }
    const { vulnOverrides, addedAgeExcludes } = await fix(filteredAuditReport, opts)
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
    info: 0,
    low: 0,
    moderate: 0,
    high: 0,
    critical: 0,
  }
  const totalVulnerabilityCount = Object.values(vulnerabilities)
    .reduce((sum: number, vulnerabilitiesCount: number) => sum + vulnerabilitiesCount, 0)
  const ignoreGhsas = opts.auditConfig?.ignoreGhsas
  if (ignoreGhsas?.length) {
    // Compare GHSA ids after normalizing so stored entries with varying
    // casing still match the canonical form on the advisory.
    const ignoreSet = new Set(ignoreGhsas.map(normalizeGhsaId))
    auditReport.advisories = pickBy(({ github_advisory_id: githubAdvisoryId, severity }) => {
      if (!ignoreSet.has(normalizeGhsaId(githubAdvisoryId))) {
        return true
      }
      ignoredVulnerabilities[severity as AuditLevelString] += 1
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
      ['Patched versions', advisory.patched_versions ?? '(unknown)'],
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

export function formatFixWithUpdateOutput (result: FixWithUpdateResult, auditReport: AuditReport): string {
  const output: string[] = []

  interface IdAndAdvisory {
    id: number
    advisory?: AuditAdvisory
  }

  /**
   * Sort the given array of advisory IDs by severity descending
   */
  function sortBySeverity (ids: number[]): IdAndAdvisory[] {
    return ids.map(id => ({ id, advisory: auditReport.advisories[id] })).sort((a, b) => {
      const aValue = a.advisory ? AUDIT_LEVEL_NUMBER[a.advisory.severity] : -1
      const bValue = b.advisory ? AUDIT_LEVEL_NUMBER[b.advisory.severity] : -1
      return bValue - aValue
    })
  }

  const fixed = sortBySeverity(result.fixed)
  const remaining = sortBySeverity(result.remaining)

  const fixedString = fixed.length === 1 ? 'vulnerability was fixed' : 'vulnerabilities were fixed'
  const remainingString = remaining.length === 1 ? 'vulnerability remains' : 'vulnerabilities remain'

  output.push(`${chalk.green(fixed.length)} ${fixedString}, ${chalk.red(remaining.length)} ${remainingString}.`)

  function summarizeAdvisory (fixed: boolean, { id, advisory }: IdAndAdvisory): string {
    if (advisory) {
      const color = fixed ? chalk.green : AUDIT_COLOR[advisory.severity]
      return `- (${color(advisory.severity)}) "${color(advisory.title)}" ${chalk.blue(advisory.module_name)}`
    }
    return `- Advisory with ID ${id} (details not found in the audit report)`
  }

  if (fixed.length > 0) {
    output.push('\nThe fixed vulnerabilities are:')
    for (const f of fixed) {
      output.push(summarizeAdvisory(true, f))
    }
  }

  if (remaining.length > 0) {
    output.push('\nThe remaining vulnerabilities are:')
    for (const r of remaining) {
      output.push(summarizeAdvisory(false, r))
    }
  }

  // Add trailing newline
  output.push('')
  return output.join('\n')
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

async function interactiveAuditFix (auditReport: AuditReport): Promise<AuditReport> {
  const choices = getAuditFixChoices(Object.values(auditReport.advisories))
  if (choices.length === 0) {
    return auditReport
  }
  const { selectedVulnerabilities } = await enquirer.prompt({
    choices,
    footer: '\nEnter to start fixing. Ctrl-c to cancel.',
    indicator (state: any, choice: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
      return ` ${choice.enabled ? '●' : '○'}`
    },
    message: 'Choose which vulnerabilities to fix ' +
      `(Press ${chalk.cyan('<space>')} to select, ` +
      `${chalk.cyan('<a>')} to toggle all, ` +
      `${chalk.cyan('<i>')} to invert selection)`,
    name: 'selectedVulnerabilities',
    pointer: '❯',
    result () {
      return this.selected
    },
    format () {
      if (!this.state.submitted || this.state.cancelled) return ''

      if (Array.isArray(this.selected)) {
        return this.selected
          .filter((choice: AuditChoiceRow) => !/^\[.+\]$/.test(choice.name))
          .map((choice: AuditChoiceRow) => this.styles.primary(choice.name)).join(', ')
      }
      return this.styles.primary(this.selected.name)
    },
    styles: {
      dark: chalk.reset,
      em: chalk.bgBlack.whiteBright,
      success: chalk.reset,
    },
    type: 'multiselect',
    validate (value: string[]) {
      if (value.length === 0) {
        return 'You must choose at least one vulnerability.'
      }
      return true
    },
    j () {
      return this.down()
    },
    k () {
      return this.up()
    },
    cancel () {
      // By default, canceling the prompt via Ctrl+c throws an empty string.
      // The custom cancel function prevents that behavior.
      // Otherwise, pnpm CLI would print an error and confuse users.
      // See related issue: https://github.com/enquirer/enquirer/issues/225
      globalInfo('Audit fix canceled')
      process.exit(0)
    },
  } as any) as any // eslint-disable-line @typescript-eslint/no-explicit-any

  const selectedKeys = new Set(
    (selectedVulnerabilities as AuditChoiceRow[]).map((c) => c.value)
  )
  const selectedAdvisories = Object.fromEntries(
    Object.entries(auditReport.advisories)
      .filter(([, advisory]) =>
        selectedKeys.has(`${advisory.module_name}@${advisory.vulnerable_versions}`)
      )
  )
  return { ...auditReport, advisories: selectedAdvisories }
}
