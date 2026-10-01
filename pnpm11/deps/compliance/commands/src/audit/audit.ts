import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes, type UniversalOptions } from '@pnpm/config.reader'
import { audit, type AuditLevelString, type AuditReport, type AuditVulnerabilityCounts, type IgnoredAuditVulnerabilityCounts, normalizeGhsaId } from '@pnpm/deps.compliance.audit'
import { PnpmError } from '@pnpm/error'
import { type InstallCommandOptions, update } from '@pnpm/installing.commands'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { RegistriesByScope } from '@pnpm/types'
import { pick, pickBy } from 'ramda'
import { renderHelp } from 'render-help'

import { type AuditContext, type AuditNetworkOptions, createAuditNetworkOptions, loadAuditContext } from './auditContext.js'
import { type AuditCommandResult, type FixMethod, runAuditFix } from './auditFix.js'
import { AUDIT_LEVEL_NUMBER, renderAdvisoryTables, reportSummary } from './auditOutput.js'
import { ignore } from './ignore.js'
import { correctInferredPatchedVersions, createPublishTimesFetcher, type PublishTimesFetcher } from './publishTimes.js'
import { auditSignatures } from './signatures.js'

export { formatFixWithUpdateOutput } from './auditOutput.js'

const AUDIT_OPTIONS_HELP = [
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
]

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
    // A plain String, not a list of allowed values and not paired with
    // Boolean: a list coerces an unexpected value to true, and Boolean makes
    // nopt swallow the next `--flag` as the fix method. A bare `--fix` then
    // arrives as the empty string.
    fix: String,
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
        title: 'Commands',

        list: [
          {
            description: 'Verify ECDSA registry signatures for installed packages from registries that provide signing keys at /-/npm/v1/keys.',
            name: 'signatures',
          },
        ],
      },
      {
        title: 'Options',

        list: AUDIT_OPTIONS_HELP,
      },
    ],
    url: docsUrl('audit'),
    usages: ['pnpm audit [options]', 'pnpm audit signatures [options]'],
  })
}

export type { PublishTimesFetcher } from './publishTimes.js'

export type AuditOptions = Pick<UniversalOptions, 'dir'> & {
  /**
   * The `--fix` value as the CLI hands it over: nopt types the option as a
   * string, so `--fix` with no method arrives as `''` and `--fix=<method>`
   * as the raw method name, valid or not. An rc file may still set a boolean.
   */
  fix?: boolean | string
  ignoreRegistryErrors?: boolean
  interactive?: boolean
  json?: boolean
  lockfileDir?: string
  registriesByScope: RegistriesByScope
  ignore?: string[]
  ignoreUnfixable?: boolean
  /**
   * Memoized packument publish-time lookup shared between patched-version
   * validation and the age-gate exclusion flow so each package is fetched at
   * most once per `pnpm audit` run. When unset, each consumer creates its own
   * fetcher.
   */
  getPublishTimes?: PublishTimesFetcher
} & Pick<Config, 'auditConfig'
| 'auditIgnorePrune'
| 'auditLevel'
| 'minimumReleaseAge'
| 'ca'
| 'cert'
| 'httpProxy'
| 'httpsProxy'
| 'key'
| 'localAddress'
| 'maxSockets'
| 'networkConcurrency'
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
| 'resolvePeersFromWorkspaceRoot'
| 'configByUri'
| 'virtualStoreDirMaxLength'
| 'workspaceDir'
> & Partial<Pick<Config, 'filter' | 'filterProd' | 'workspaceRoot'>> & Pick<ConfigContext,
| 'rootProjectManifest'
| 'rootProjectManifestDir'
> & InstallCommandOptions

const DEFAULT_FIX_METHOD = 'override'

export function handler (opts: AuditOptions): Promise<AuditCommandResult>
export function handler (opts: AuditOptions, params: string[]): Promise<AuditCommandResult>
export async function handler (opts: AuditOptions, params: string[] = []): Promise<AuditCommandResult> {
  if (params.length > 0) {
    return runAuditSubcommand(opts, params)
  }
  const auditContext = await loadAuditContext(opts)
  const networkOptions = createAuditNetworkOptions(opts)
  let auditReport!: AuditReport
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri)
  try {
    auditReport = await audit(auditContext.lockfile, getAuthHeader, createAuditRequestOptions(opts, { auditContext, networkOptions }))
  } catch (err: any) { // eslint-disable-line
    if (opts.ignoreRegistryErrors) {
      return {
        exitCode: 0,
        output: err.message,
      }
    }

    throw err
  }
  // The inferred patched range is syntactic: verify a published version
  // actually satisfies it before the report and any fix flow can claim one.
  // One fetcher is shared with the fix flows below so each affected package
  // is requested at most once.
  const getPublishTimes = createPublishTimesFetcher(opts)
  await correctInferredPatchedVersions(auditReport.advisories, getPublishTimes)
  const fixMethod = resolveFixMethod(opts)
  if (fixMethod != null) {
    return runAuditFix(auditReport, opts, { fixMethod, getPublishTimes, include: auditContext.include })
  }
  if (opts.ignore !== undefined || opts.ignoreUnfixable) {
    return ignoreVulnerabilities(auditReport, opts)
  }
  return reportVulnerabilities(auditReport, opts)
}

function runAuditSubcommand (opts: AuditOptions, params: string[]): Promise<AuditCommandResult> {
  if (params[0] === 'signatures') {
    if (params.length > 1) {
      throw new PnpmError('AUDIT_UNKNOWN_SUBCOMMAND', `Unknown audit subcommand: ${params.slice(0, 2).join(' ')}`)
    }
    return auditSignatures(opts)
  }
  throw new PnpmError('AUDIT_UNKNOWN_SUBCOMMAND', `Unknown audit subcommand: ${params[0]}`)
}

function createAuditRequestOptions (
  opts: AuditOptions,
  { auditContext, networkOptions }: { auditContext: AuditContext, networkOptions: AuditNetworkOptions }
): Parameters<typeof audit>[2] {
  return {
    dispatcherOptions: {
      ca: networkOptions.ca,
      cert: networkOptions.cert,
      httpProxy: networkOptions.httpProxy,
      httpsProxy: networkOptions.httpsProxy,
      key: networkOptions.key,
      localAddress: networkOptions.localAddress,
      maxSockets: networkOptions.maxSockets,
      noProxy: networkOptions.noProxy,
      strictSsl: networkOptions.strictSsl,
      timeout: networkOptions.fetchTimeout,
    },
    envLockfile: auditContext.envLockfile,
    include: auditContext.include,
    registry: opts.registriesByScope.default,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    retry: networkOptions.retry,
    timeout: networkOptions.fetchTimeout,
  }
}

function resolveFixMethod (opts: Pick<AuditOptions, 'fix' | 'interactive'>): FixMethod | undefined {
  const { fix: fixOption } = opts
  if (fixOption === 'update' || fixOption === 'override') {
    return fixOption
  }
  if (isFixWithoutMethod(fixOption) || (opts.interactive && !fixOption)) {
    return DEFAULT_FIX_METHOD
  }
  if (!fixOption) {
    return undefined
  }
  throw new PnpmError('INVALID_FIX_OPTION', `Invalid value for --fix: ${fixOption}. Should be one of "override" or "update"`)
}

async function ignoreVulnerabilities (auditReport: AuditReport, opts: AuditOptions): Promise<AuditCommandResult> {
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

function reportVulnerabilities (auditReport: AuditReport, opts: AuditOptions): AuditCommandResult {
  const ignoredVulnerabilities = removeIgnoredAdvisories(auditReport, opts.auditConfig?.ignoreGhsas)
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

  const output = renderAdvisoryTables(advisoryEntries)
  const vulnerabilities: AuditVulnerabilityCounts = { info: 0, low: 0, moderate: 0, high: 0, critical: 0 }
  for (const { severity } of Object.values(auditReport.advisories)) {
    vulnerabilities[severity] += 1
  }
  return {
    exitCode: output ? 1 : 0,
    output: `${output}${reportSummary(vulnerabilities, ignoredVulnerabilities)}`,
  }
}

/**
 * Removes the advisories listed in `ignoreGhsas` from `auditReport` and
 * returns how many were removed per severity.
 */
function removeIgnoredAdvisories (auditReport: AuditReport, ignoreGhsas: string[] | undefined): IgnoredAuditVulnerabilityCounts {
  const ignoredVulnerabilities: IgnoredAuditVulnerabilityCounts = {
    info: 0,
    low: 0,
    moderate: 0,
    high: 0,
    critical: 0,
  }
  if (!ignoreGhsas?.length) return ignoredVulnerabilities
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
  return ignoredVulnerabilities
}

/**
 * Whether `--fix` was passed without a fix method, in which case
 * {@link DEFAULT_FIX_METHOD} applies. The CLI spells that as the empty
 * string; an rc file spells it as a boolean or its string form.
 */
function isFixWithoutMethod (fix: AuditOptions['fix']): boolean {
  return fix === '' || fix === true || fix === 'true'
}
