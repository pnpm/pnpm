import type { Log } from '@pnpm/core-loggers'
import { renderPeerIssues } from '@pnpm/deps.inspection.peers-issues-renderer'
import type { PnpmError } from '@pnpm/error'
import { renderDedupeCheckIssues } from '@pnpm/installing.dedupe.issues-renderer'
import type { DedupeCheckIssues } from '@pnpm/installing.dedupe.types'
import type { PeerDependencyIssuesByProjects } from '@pnpm/types'
import chalk from 'chalk'
import { equals } from 'ramda'
import StackTracey from 'stacktracey'

import { EOL } from './constants.js'
import type { ReporterPnpmConfig } from './ReporterPnpmConfig.js'
import {
  type ErrorInfo,
  reportLockfileBreakingChange,
  reportModifiedDependency,
  reportModulesBreakingChange,
  reportStoreBreakingChange,
  reportUnexpectedStore,
  reportUnexpectedVirtualStoreDir,
} from './reportInstallStateErrors.js'

StackTracey.maxColumnWidths = {
  callee: 25,
  file: 350,
  sourceLine: 25,
}

const highlight = chalk.yellow

export function reportError (logObj: Log, config?: ReporterPnpmConfig): string | null {
  const errorInfo = getErrorInfo(logObj, config)
  if (!errorInfo) return null
  let output = formatErrorSummary(errorInfo.title, (logObj as LogObjWithPossibleError).err?.code)
  if (logObj.pkgsStack != null) {
    if (logObj.pkgsStack.length > 0) {
      output += `\n\n${formatPkgsStack(logObj.pkgsStack)}`
    } else if ('prefix' in logObj && logObj.prefix) {
      output += `\n\nThis error happened while installing a direct dependency of ${logObj.prefix}`
    }
  }
  if (errorInfo.body) {
    output += `\n\n${errorInfo.body}`
  }
  return output

  /**
   * A type to assist with introspection of the logObj.
   * These objects may or may not have an `err` field.
   */
  interface LogObjWithPossibleError {
    readonly err?: { code?: string }
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- the error code determines the extra fields on the log, which the Log type does not carry
type LogWithErrorFields = any

type ErrorInfoReporter = (
  err: PnpmError & { stack: object },
  logObj: LogWithErrorFields,
  config?: ReporterPnpmConfig
) => ErrorInfo

const ERROR_INFO_REPORTERS = new Map<string, ErrorInfoReporter>([
  ['ERR_PNPM_UNEXPECTED_STORE', (err, logObj) => reportUnexpectedStore(err, logObj)],
  ['ERR_PNPM_UNEXPECTED_VIRTUAL_STORE', (err, logObj) => reportUnexpectedVirtualStoreDir(err, logObj)],
  ['ERR_PNPM_STORE_BREAKING_CHANGE', (_err, logObj) => reportStoreBreakingChange(logObj)],
  ['ERR_PNPM_MODULES_BREAKING_CHANGE', (_err, logObj) => reportModulesBreakingChange(logObj)],
  ['ERR_PNPM_MODIFIED_DEPENDENCY', (_err, logObj) => reportModifiedDependency(logObj)],
  ['ERR_PNPM_LOCKFILE_BREAKING_CHANGE', (err, logObj) => reportLockfileBreakingChange(err, logObj)],
  ['ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT', (err) => ({ title: err.message })],
  ['ERR_PNPM_MISSING_TIME', (err) => ({ title: err.message, body: 'If you cannot fix this registry issue, then set "resolution-mode" to "highest".' })],
  ['ERR_PNPM_NO_MATCHING_VERSION', (err, logObj) => formatNoMatchingVersion(err, logObj)],
  ['ERR_PNPM_NO_MATURE_MATCHING_VERSION', (err, logObj) => formatNoMatchingVersion(err, logObj)],
  ['ERR_PNPM_RECURSIVE_FAIL', (_err, logObj) => formatRecursiveCommandSummary(logObj)],
  ['ERR_PNPM_BAD_TARBALL_SIZE', (err, logObj) => reportBadTarballSize(err, logObj)],
  ['ELIFECYCLE', (_err, logObj) => reportLifecycleError(logObj)],
  ['ERR_PNPM_UNSUPPORTED_ENGINE', (_err, logObj) => reportEngineError(logObj)],
  ['ERR_PNPM_PEER_DEP_ISSUES', (err, logObj) => reportPeerDependencyIssuesError(err, logObj)],
  ['ERR_PNPM_DEDUPE_CHECK_ISSUES', (err, logObj) => reportDedupeCheckIssuesError(err, logObj)],
  ['ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER', (err, logObj) => reportSpecNotSupportedByAnyResolverError(err, logObj)],
  ['ERR_PNPM_FETCH_401', (err, logObj, config) => reportAuthError(err, logObj, config)],
  ['ERR_PNPM_FETCH_403', (err, logObj, config) => reportAuthError(err, logObj, config)],
])

function getErrorInfo (logObj: Log, config?: ReporterPnpmConfig): ErrorInfo | null {
  if (!('err' in logObj && logObj.err)) {
    return { title: logObj.message! }
  }
  const err = logObj.err as (PnpmError & { stack: object })
  const reportErrorInfo = ERROR_INFO_REPORTERS.get(err.code)
  if (reportErrorInfo != null) {
    return reportErrorInfo(err, logObj, config)
  }
  // Errors with unknown error codes are printed with stack trace
  if (!err.code?.startsWith?.('ERR_PNPM_')) {
    return formatGenericError(err.message ?? (logObj as { message: string }).message, err.stack)
  }
  return {
    title: err.message ?? '',
    body: (logObj as { hint?: string }).hint,
  }
}

interface PkgStackItem {
  readonly id: string
  readonly name: string
  // The version may be missing if this was a private workspace package without
  // the version field set.
  readonly version?: string
}

function formatPkgNameVer ({ name, version }: PkgStackItem) {
  return version == null
    ? name
    : `${name}@${version}`
}

function formatPkgsStack (pkgsStack: readonly PkgStackItem[]) {
  return `This error happened while installing the dependencies of \
${formatPkgNameVer(pkgsStack[0])}\
${pkgsStack.slice(1).map((pkgInfo) => `${EOL} at ${formatPkgNameVer(pkgInfo)}`).join('')}`
}

interface PackageMeta {
  name: string
  'dist-tags': Record<string, string> & {
    latest: string
  }
  versions: Record<string, object>
  time?: Record<string, string>
}

function formatNoMatchingVersion (err: Error, msg: { packageMeta?: PackageMeta }): ErrorInfo {
  // Errors raised by the install/dlx/self-update layer after the resolver
  // surfaces violations may not carry the original packageMeta. In that
  // case the error message alone already names every offending entry,
  // so we just echo it through without the registry-metadata appendix.
  const meta = msg.packageMeta
  if (!meta) {
    return { title: err.message }
  }
  const latestVersion = meta['dist-tags'].latest
  let output = `The latest release of ${meta.name} is "${latestVersion}".`
  const latestTime = meta.time?.[latestVersion]
  if (latestTime) {
    output += ` Published at ${stringifyDate(latestTime)}`
  }
  output += EOL

  if (!equals(Object.keys(meta['dist-tags']), ['latest'])) {
    output += EOL + 'Other releases are:' + EOL + formatOtherReleases(meta)
  }

  output += `${EOL}If you need the full list of all ${Object.keys(meta.versions).length} published versions run "pnpm view ${meta.name} versions".`

  return {
    title: err.message,
    body: output,
  }
}

function formatOtherReleases (meta: PackageMeta): string {
  let output = ''
  for (const tag in meta['dist-tags']) {
    if (tag === 'latest') continue
    const version = meta['dist-tags'][tag]
    output += `  * ${tag}: ${version}`
    const time = meta.time?.[version]
    if (time) {
      output += ` published at ${stringifyDate(time)}`
    }
    output += EOL
  }
  return output
}

function stringifyDate (dateStr: string): string {
  const now = Date.now()
  const oneDayAgo = now - 24 * 60 * 60 * 1000
  const date = new Date(dateStr)
  if (date.getTime() < oneDayAgo) {
    return date.toLocaleDateString()
  }
  return `${date.toLocaleDateString()} ${date.toLocaleTimeString()}`
}

function formatGenericError (errorMessage: string, stack: object): ErrorInfo {
  if (stack) {
    let prettyStack: string | undefined
    try {
      prettyStack = new StackTracey(stack).asTable()
    } catch {
      prettyStack = stack.toString()
    }
    if (prettyStack) {
      return {
        title: errorMessage,
        body: prettyStack,
      }
    }
  }
  return { title: errorMessage }
}

function formatErrorSummary (message: string, code?: string): string {
  return `${chalk.bgRed.red('[')}${chalk.bgRed.black(code ?? 'ERROR')}${chalk.bgRed.red(']')} ${chalk.red(message)}`
}

function formatRecursiveCommandSummary (msg: { failures: Array<Error & { prefix: string }>, passes: number }): ErrorInfo {
  const output = EOL + `Summary: ${chalk.red(`${msg.failures.length} fails`)}, ${msg.passes} passes` + EOL + EOL +
    msg.failures.map(({ message, prefix }) => {
      return prefix + ':' + EOL + formatErrorSummary(message)
    }).join(EOL + EOL)
  return {
    title: '',
    body: output,
  }
}

function reportBadTarballSize (err: Error, _msg: object): ErrorInfo {
  return {
    title: err.message,
    body: `Seems like you have internet connection issues.
Try running the same command again.
If that doesn't help, try one of the following:

- Set a bigger value for the \`fetch-retries\` config.
    To check the current value of \`fetch-retries\`, run \`pnpm get fetch-retries\`.
    To set a new value, run \`pnpm set fetch-retries <number>\`.

- Set \`network-concurrency\` to 1.
    This change will slow down installation times, so it is recommended to
    delete the config once the internet connection is good again: \`pnpm config delete network-concurrency\`

NOTE: You may also override configs via flags.
For instance, \`pnpm install --fetch-retries 5 --network-concurrency 1\``,
  }
}

function reportLifecycleError (
  msg: {
    stage: string
    errno?: number | string
    signal?: string
  }
): ErrorInfo {
  if (msg.signal) {
    return { title: `Command failed with signal ${msg.signal}.` }
  }
  if (msg.stage === 'test') {
    return { title: 'Test failed. See above for more details.' }
  }
  if (typeof msg.errno === 'number') {
    return { title: `Command failed with exit code ${msg.errno}.` }
  }
  return { title: 'Command failed.' }
}

function reportEngineError (
  msg: {
    message: string
    current: {
      node: string
      pnpm: string
    }
    packageId: string
    wanted: {
      node?: string
      pnpm?: string
    }
  }
): ErrorInfo {
  let output = ''
  if (msg.wanted.pnpm) {
    output += `\
Your pnpm version is incompatible with "${msg.packageId}".

Expected version: ${msg.wanted.pnpm}
Got: ${msg.current.pnpm}

This is happening because the package's manifest has an engines.pnpm field specified.
To fix this issue, install the required pnpm version globally.

To install the latest version of pnpm, run "pnpm i -g pnpm".
To check your pnpm version, run "pnpm -v".`
  }
  if (msg.wanted.node) {
    if (output) output += EOL + EOL
    output += `\
Your Node version is incompatible with "${msg.packageId}".

Expected version: ${msg.wanted.node}
Got: ${msg.current.node}

This is happening because the package's manifest has an engines.node field specified.
To fix this issue, install the required Node version.`
  }
  return {
    title: 'Unsupported environment (bad pnpm and/or Node.js version)',
    body: output,
  }
}

function reportAuthError (
  err: Error,
  msg: { hint?: string },
  config?: ReporterPnpmConfig
): ErrorInfo {
  const foundSettings = [] as string[]
  for (const [key, value] of Object.entries(config?.authConfig ?? {})) {
    if (key[0] === '@') {
      foundSettings.push(`${key}=${String(value)}`)
      continue
    }
    if (
      key.endsWith('_auth') ||
      key.endsWith('_authToken') ||
      key.endsWith('username') ||
      key.endsWith('_password')
    ) {
      foundSettings.push(`${key}=${hideSecureInfo(key, value)}`)
    }
  }
  let output = msg.hint ? `${msg.hint}${EOL}${EOL}` : ''

  if (foundSettings.length === 0) {
    output += `No authorization settings were found in the configs.
Try to log in to the registry by running "pnpm login"
or add the auth tokens manually to the ~/.npmrc file.`
  } else {
    output += `These authorization settings were found:
${foundSettings.join('\n')}`
  }
  return {
    title: err.message,
    body: output,
  }
}

function hideSecureInfo (key: string, value: string): string {
  if (key.endsWith('_password')) return '[hidden]'
  if (key.endsWith('_auth') || key.endsWith('_authToken')) return `${value.substring(0, 4)}[hidden]`
  return value
}

function reportPeerDependencyIssuesError (
  err: Error,
  msg: { issuesByProjects: PeerDependencyIssuesByProjects }
): ErrorInfo {
  const hasMissingPeers = getHasMissingPeers(msg.issuesByProjects)
  const hints: string[] = []
  if (hasMissingPeers) {
    hints.push(`To auto-install peer dependencies, add the following to "pnpm-workspace.yaml" in your project root:

  autoInstallPeers: true`)
  }
  hints.push(`To disable failing on peer dependency issues, add the following to pnpm-workspace.yaml in your project root:

  strictPeerDependencies: false`)
  const formattedHints = hints.map((hint) => `hint: ${hint}`).join('\n')
  const rendered = renderPeerIssues(msg.issuesByProjects)
  return {
    title: err.message,
    body: rendered ? `${rendered}\n${formattedHints}` : formattedHints,
  }
}

function getHasMissingPeers (issuesByProjects: PeerDependencyIssuesByProjects): boolean {
  return Object.values(issuesByProjects)
    .some((issues) => Object.values(issues.missing).flat().some(({ optional }) => !optional))
}

function reportDedupeCheckIssuesError (err: Error, msg: { dedupeCheckIssues: DedupeCheckIssues }): ErrorInfo {
  return {
    title: err.message,
    body: `\
${renderDedupeCheckIssues(msg.dedupeCheckIssues)}
Run ${chalk.yellow('pnpm dedupe')} to apply the changes above.
`,
  }
}

function reportSpecNotSupportedByAnyResolverError (err: Error, logObj: Log): ErrorInfo {
  // If the catalog protocol specifier was sent to a "real resolver", it'll
  // eventually throw a "specifier not supported" error since the catalog
  // protocol is meant to be replaced before it's passed to any of the real
  // resolvers.
  //
  // If this kind of error is thrown, and the dependency bareSpecifier is using the
  // catalog protocol it's most likely because we're trying to install an out of
  // repo dependency that was published incorrectly. For example, it may be been
  // mistakenly published with 'npm publish' instead of 'pnpm publish'. Report a
  // more clear error in this case.
  if (logObj.package?.bareSpecifier?.startsWith('catalog:')) {
    return reportExternalCatalogProtocolError(err, logObj)
  }

  return {
    title: err.message ?? '',
    body: logObj.hint,
  }
}

function reportExternalCatalogProtocolError (err: Error, logObj: Log): ErrorInfo {
  const { pkgsStack } = logObj
  const problemDep = pkgsStack?.[0]

  let body = `\
An external package outside of the pnpm workspace declared a dependency using
the catalog protocol. This is likely a bug in that external package. Only
packages within the pnpm workspace may use catalogs. Usages of the catalog
protocol are replaced with real specifiers on 'pnpm publish'.
`

  if (problemDep != null) {
    body += `\

This is likely a bug in the publishing automation of this package. Consider filing
a bug with the authors of:

  ${highlight(formatPkgNameVer(problemDep))}
`
  }

  return {
    title: err.message,
    body,
  }
}
