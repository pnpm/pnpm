import { type Config } from '@pnpm/config'
import { type Log } from '@pnpm/core-loggers'
import { renderDedupeCheckIssues } from '@pnpm/dedupe.issues-renderer'
import { type DedupeCheckIssues } from '@pnpm/dedupe.types'
import { type PnpmError } from '@pnpm/error'
import { renderPeerIssues } from '@pnpm/render-peer-issues'
import { type PeerDependencyRules, type PeerDependencyIssuesByProjects } from '@pnpm/types'
import chalk from 'chalk'
import equals from 'ramda/src/equals'
import StackTracey from 'stacktracey'
import { EOL } from './constants'

StackTracey.maxColumnWidths = {
  callee: 25,
  file: 350,
  sourceLine: 25,
}

const highlight = chalk.yellow
const colorPath = chalk.gray

export function reportError (logObj: Log, config?: Config, peerDependencyRules?: PeerDependencyRules): string | null {
  const errorInfo = getErrorInfo(logObj, config, peerDependencyRules)
  if (!errorInfo) return null
  let output = formatErrorSummary(errorInfo.title, (logObj as LogObjWithPossibleError).err?.code)
  if (logObj['pkgsStack'] != null) {
    if (logObj['pkgsStack'].length > 0) {
      output += `\n\n${formatPkgsStack(logObj['pkgsStack'])}`
    } else if (logObj['prefix']) {
      output += `\n\nThis error happened while installing a direct dependency of ${logObj['prefix'] as string}`
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

interface ErrorInfo {
  title: string
  body?: string
}

function getErrorInfo (logObj: Log, config?: Config, peerDependencyRules?: PeerDependencyRules): ErrorInfo | null {
  if (logObj['err']) {
    const err = logObj['err'] as (PnpmError & { stack: object })
    switch (err.code) {
    case 'ERR_PNPM_UNEXPECTED_STORE':
      return reportUnexpectedStore(err, logObj as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    case 'ERR_PNPM_UNEXPECTED_VIRTUAL_STORE':
      return reportUnexpectedVirtualStoreDir(err, logObj as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    case 'ERR_PNPM_STORE_BREAKING_CHANGE':
      return reportStoreBreakingChange(logObj as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    case 'ERR_PNPM_MODULES_BREAKING_CHANGE':
      return reportModulesBreakingChange(logObj as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    case 'ERR_PNPM_MODIFIED_DEPENDENCY':
      return reportModifiedDependency(logObj as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    case 'ERR_PNPM_LOCKFILE_BREAKING_CHANGE':
      return reportLockfileBreakingChange(err, logObj)
    case 'ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT':
      return { title: err.message }
    case 'ERR_PNPM_MISSING_TIME':
      return { title: err.message, body: 'If you cannot fix this registry issue, then set "resolution-mode" to "highest".' }
    case 'ERR_PNPM_NO_MATCHING_VERSION':
      return formatNoMatchingVersion(err, logObj)
    case 'ERR_PNPM_RECURSIVE_FAIL':
      return formatRecursiveCommandSummary(logObj as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    case 'ERR_PNPM_BAD_TARBALL_SIZE':
      return reportBadTarballSize(err, logObj)
    case 'ELIFECYCLE':
      return reportLifecycleError(logObj as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    case 'ERR_PNPM_UNSUPPORTED_ENGINE':
      return reportEngineError(logObj as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    case 'ERR_PNPM_PEER_DEP_ISSUES':
      return reportPeerDependencyIssuesError(err, logObj as any, peerDependencyRules) // eslint-disable-line @typescript-eslint/no-explicit-any
    case 'ERR_PNPM_DEDUPE_CHECK_ISSUES':
      return reportDedupeCheckIssuesError(err, logObj as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    case 'ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER':
      return reportSpecNotSupportedByAnyResolverError(err, logObj as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    case 'ERR_PNPM_FETCH_401':
    case 'ERR_PNPM_FETCH_403':
      return reportAuthError(err, logObj as any, config) // eslint-disable-line @typescript-eslint/no-explicit-any
    default: {
      // Errors with unknown error codes are printed with stack trace
      if (!err.code?.startsWith?.('ERR_PNPM_')) {
        return formatGenericError(err.message ?? logObj['message'], err.stack)
      }
      return {
        title: err.message ?? '',
        body: logObj['hint'],
      }
    }
    }
  }
  return { title: logObj['message'] }
}

function formatPkgsStack (pkgsStack: Array<{ id: string, name: string, version: string }>) {
  return `This error happened while installing the dependencies of \
${pkgsStack[0].name}@${pkgsStack[0].version}\
${pkgsStack.slice(1).map(({ name, version }) => `${EOL} at ${name}@${version}`).join('')}`
}

function formatNoMatchingVersion (err: Error, msg: object) {
  const meta: {
    name: string
    'dist-tags': Record<string, string> & { latest: string }
    versions: Record<string, object>
  } = msg['packageMeta']
  let output = `The latest release of ${meta.name} is "${meta['dist-tags'].latest}".${EOL}`

  if (!equals(Object.keys(meta['dist-tags']), ['latest'])) {
    output += EOL + 'Other releases are:' + EOL
    for (const tag in meta['dist-tags']) {
      if (tag !== 'latest') {
        output += `  * ${tag}: ${meta['dist-tags'][tag]}${EOL}`
      }
    }
  }

  output += `${EOL}If you need the full list of all ${Object.keys(meta.versions).length} published versions run "$ pnpm view ${meta.name} versions".`

  return {
    title: err.message,
    body: output,
  }
}

function reportUnexpectedStore (
  err: Error,
  msg: {
    actualStorePath: string
    expectedStorePath: string
    modulesDir: string
  }
): ErrorInfo {
  return {
    title: err.message,
    body: `The dependencies at "${msg.modulesDir}" are currently linked from the store at "${msg.expectedStorePath}".

pnpm now wants to use the store at "${msg.actualStorePath}" to link dependencies.

If you want to use the new store location, reinstall your dependencies with "pnpm install".

You may change the global store location by running "pnpm config set store-dir <dir> --global".
(This error may happen if the node_modules was installed with a different major version of pnpm)`,
  }
}

function reportUnexpectedVirtualStoreDir (
  err: Error,
  msg: {
    actual: string
    expected: string
    modulesDir: string
  }
): ErrorInfo {
  return {
    title: err.message,
    body: `The dependencies at "${msg.modulesDir}" are currently symlinked from the virtual store directory at "${msg.expected}".

pnpm now wants to use the virtual store at "${msg.actual}" to link dependencies from the store.

If you want to use the new virtual store location, reinstall your dependencies with "pnpm install".

You may change the virtual store location by changing the value of the virtual-store-dir config.`,
  }
}

function reportStoreBreakingChange (msg: {
  additionalInformation?: string
  storePath: string
  relatedIssue?: number
  relatedPR?: number
}): ErrorInfo {
  let output = `Store path: ${colorPath(msg.storePath)}

Run "pnpm install" to recreate node_modules.`

  if (msg.additionalInformation) {
    output = `${output}${EOL}${EOL}${msg.additionalInformation}`
  }

  output += formatRelatedSources(msg)
  return {
    title: 'The store used for the current node_modules is incompatible with the current version of pnpm',
    body: output,
  }
}

function reportModulesBreakingChange (msg: {
  additionalInformation?: string
  modulesPath: string
  relatedIssue?: number
  relatedPR?: number
}): ErrorInfo {
  let output = `node_modules path: ${colorPath(msg.modulesPath)}

Run ${highlight('pnpm install')} to recreate node_modules.`

  if (msg.additionalInformation) {
    output = `${output}${EOL}${EOL}${msg.additionalInformation}`
  }

  output += formatRelatedSources(msg)
  return {
    title: 'The current version of pnpm is not compatible with the available node_modules structure',
    body: output,
  }
}

function formatRelatedSources (msg: {
  relatedIssue?: number
  relatedPR?: number
}): string {
  let output = ''

  if (!msg.relatedIssue && !msg.relatedPR) return output

  output += EOL

  if (msg.relatedIssue) {
    output += EOL + `Related issue: ${colorPath(`https://github.com/pnpm/pnpm/issues/${msg.relatedIssue}`)}`
  }

  if (msg.relatedPR) {
    output += EOL + `Related PR: ${colorPath(`https://github.com/pnpm/pnpm/pull/${msg.relatedPR}`)}`
  }

  return output
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
  return `${chalk.bgRed.black(`\u2009${code ?? 'ERROR'}\u2009`)} ${chalk.red(message)}`
}

function reportModifiedDependency (msg: { modified: string[] }): ErrorInfo {
  return {
    title: 'Packages in the store have been mutated',
    body: `These packages are modified:
${msg.modified.map((pkgPath: string) => colorPath(pkgPath)).join(EOL)}

You can run ${highlight('pnpm install --force')} to refetch the modified packages`,
  }
}

function reportLockfileBreakingChange (err: Error, msg: object): ErrorInfo {
  return {
    title: err.message,
    body: `Run with the ${highlight('--force')} parameter to recreate the lockfile.`,
  }
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

function reportBadTarballSize (err: Error, msg: object): ErrorInfo {
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
  }
): ErrorInfo {
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
  config?: Config
): ErrorInfo {
  const foundSettings = [] as string[]
  for (const [key, value] of Object.entries(config?.rawConfig ?? {})) {
    if (key[0] === '@') {
      foundSettings.push(`${key}=${value}`)
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
  msg: { issuesByProjects: PeerDependencyIssuesByProjects },
  peerDependencyRules?: PeerDependencyRules
): ErrorInfo | null {
  const hasMissingPeers = getHasMissingPeers(msg.issuesByProjects)
  const hints: string[] = []
  if (hasMissingPeers) {
    hints.push('If you want peer dependencies to be automatically installed, add "auto-install-peers=true" to an .npmrc file at the root of your project.')
  }
  hints.push('If you don\'t want pnpm to fail on peer dependency issues, add "strict-peer-dependencies=false" to an .npmrc file at the root of your project.')
  const rendered = renderPeerIssues(msg.issuesByProjects, { rules: peerDependencyRules })
  if (!rendered) return null
  return {
    title: err.message,
    body: `${rendered}
${hints.map((hint) => `hint: ${hint}`).join('\n')}
`,
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
  // If this kind of error is thrown, and the dependency pref is using the
  // catalog protocol it's most likely because we're trying to install an out of
  // repo dependency that was published incorrectly. For example, it may be been
  // mistakenly published with 'npm publish' instead of 'pnpm publish'. Report a
  // more clear error in this case.
  if (logObj['package']?.['pref']?.startsWith('catalog:')) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return reportExternalCatalogProtocolError(err, logObj as any)
  }

  return {
    title: err.message ?? '',
    body: logObj['hint'],
  }
}

function reportExternalCatalogProtocolError (err: Error, logObj: Log): ErrorInfo {
  const pkgsStack: Array<{ id: string, name: string, version: string }> | undefined = logObj['pkgsStack']
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

  ${highlight(`${problemDep.name}@${problemDep.version}`)}
`
  }

  return {
    title: err.message,
    body,
  }
}
