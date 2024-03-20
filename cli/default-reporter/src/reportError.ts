import chalk from 'chalk'
import StackTracey from 'stacktracey'
import equals from 'ramda/src/equals'

import type {
  Log,
  Config,
  DedupeCheckIssues,
  PeerDependencyRules,
  PeerDependencyIssuesByProjects,
} from '@pnpm/types'
import { renderPeerIssues } from '@pnpm/render-peer-issues'
import { renderDedupeCheckIssues } from '@pnpm/dedupe.issues-renderer'

import { EOL } from './constants'

StackTracey.maxColumnWidths = {
  callee: 25,
  file: 350,
  sourceLine: 25,
}

const highlight = chalk.yellow
const colorPath = chalk.gray

export function reportError(
  logObj: Log,
  config?: Config | undefined,
  peerDependencyRules?: PeerDependencyRules | undefined
): string | null {
  const errorInfo = getErrorInfo(logObj, config, peerDependencyRules)

  if (!errorInfo) {
    return null
  }

  let output = formatErrorSummary(
    errorInfo.title,
    (logObj as LogObjWithPossibleError).err?.code
  )

  // @ts-ignore
  if (logObj.pkgsStack != null) {
    // @ts-ignore
    if (logObj.pkgsStack.length > 0) {
      // @ts-ignore
      output += `\n\n${formatPkgsStack(logObj.pkgsStack)}`
      // @ts-ignore
    } else if (logObj.prefix) {
      // @ts-ignore
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
    readonly err?: { code?: string | undefined } | undefined
  }
}

function getErrorInfo(
  logObj: Log,
  config?: Config | undefined,
  peerDependencyRules?: PeerDependencyRules | undefined
): {
  title: string
  body?: string | undefined
} | null {
  if ('err' in logObj && logObj.err instanceof Error) {
    const err = logObj.err

    if (
      'code' in err && typeof err.code === 'string' &&
      'message' in err && typeof err.message === 'string' &&
      'name' in err && typeof err.name === 'string') {
      switch (err.code) {
        case 'ERR_PNPM_UNEXPECTED_STORE': {
          // @ts-ignore
          return reportUnexpectedStore(err, logObj)
        }

        case 'ERR_PNPM_UNEXPECTED_VIRTUAL_STORE': {
          // @ts-ignore
          return reportUnexpectedVirtualStoreDir(err, logObj)
        }

        case 'ERR_PNPM_STORE_BREAKING_CHANGE': {
          // @ts-ignore
          return reportStoreBreakingChange(logObj)
        }

        case 'ERR_PNPM_MODULES_BREAKING_CHANGE': {
          // @ts-ignore
          return reportModulesBreakingChange(logObj)
        }

        case 'ERR_PNPM_MODIFIED_DEPENDENCY': {
          // @ts-ignore
          return reportModifiedDependency(logObj)
        }

        case 'ERR_PNPM_LOCKFILE_BREAKING_CHANGE': {
          return reportLockfileBreakingChange(err, logObj)
        }

        case 'ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT': {
          return { title: err.message }
        }

        case 'ERR_PNPM_NO_MATCHING_VERSION': {
          // @ts-ignore
          return formatNoMatchingVersion(err, logObj)
        }

        case 'ERR_PNPM_RECURSIVE_FAIL': {
          // @ts-ignore
          return formatRecursiveCommandSummary(logObj)
        }

        case 'ERR_PNPM_BAD_TARBALL_SIZE': {
          // @ts-ignore
          return reportBadTarballSize(err, logObj)
        }

        case 'ELIFECYCLE': {
          // @ts-ignore
          return reportLifecycleError(logObj)
        }

        case 'ERR_PNPM_UNSUPPORTED_ENGINE': {
          // @ts-ignore
          return reportEngineError(logObj)
        }

        case 'ERR_PNPM_PEER_DEP_ISSUES': {
          return reportPeerDependencyIssuesError(
            err,
            // @ts-ignore
            logObj,
            peerDependencyRules
          )
        }

        case 'ERR_PNPM_DEDUPE_CHECK_ISSUES': {
          // @ts-ignore
          return reportDedupeCheckIssuesError(err, logObj)
        }

        case 'ERR_PNPM_FETCH_401':
        case 'ERR_PNPM_FETCH_403': {
          // @ts-ignore
          return reportAuthError(err, logObj, config)
        }

        default: {
          // Errors with unknown error codes are printed with stack trace
          if (!err.code?.startsWith?.('ERR_PNPM_')) {
            // @ts-ignore
            return formatGenericError(err.message ?? logObj.message, err.stack)
          }
          return {
            title: err.message ?? '',
            // @ts-ignore
            body: logObj.hint,
          }
        }
      }
    }
  }

  // @ts-ignore
  return { title: logObj.message }
}

function formatPkgsStack(
  pkgsStack: Array<{ id: string; name: string; version: string }>
): string {
  return `This error happened while installing the dependencies of \
${pkgsStack[0].name}@${pkgsStack[0].version}\
${pkgsStack
    .slice(1)
    .map(({ name, version }) => `${EOL} at ${name}@${version}`)
    .join('')}`
}

function formatNoMatchingVersion(err: Error, msg: {
  packageMeta: {
    name: string
    'dist-tags': Record<string, string> & { latest: string }
    versions: Record<string, object>
  }}): {
    title: string;
    body: string;
  } {
  const meta = msg.packageMeta

  let output = `The latest release of ${meta.name} is "${meta['dist-tags'].latest}".${EOL}`

  if (!equals(Object.keys(meta['dist-tags']), ['latest'])) {
    output += `${EOL}Other releases are:${EOL}`

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

function reportUnexpectedStore(
  err: Error,
  msg: {
    actualStorePath: string
    expectedStorePath: string
    modulesDir: string
  }
): {
    title: string;
    body: string;
  } {
  return {
    title: err.message,
    body: `The dependencies at "${msg.modulesDir}" are currently linked from the store at "${msg.expectedStorePath}".

pnpm now wants to use the store at "${msg.actualStorePath}" to link dependencies.

If you want to use the new store location, reinstall your dependencies with "pnpm install".

You may change the global store location by running "pnpm config set store-dir <dir> --global".
(This error may happen if the node_modules was installed with a different major version of pnpm)`,
  }
}

function reportUnexpectedVirtualStoreDir(
  err: Error,
  msg: {
    actual: string
    expected: string
    modulesDir: string
  }
) {
  return {
    title: err.message,
    body: `The dependencies at "${msg.modulesDir}" are currently symlinked from the virtual store directory at "${msg.expected}".

pnpm now wants to use the virtual store at "${msg.actual}" to link dependencies from the store.

If you want to use the new virtual store location, reinstall your dependencies with "pnpm install".

You may change the virtual store location by changing the value of the virtual-store-dir config.`,
  }
}

function reportStoreBreakingChange(msg: {
  additionalInformation?: string | undefined
  storePath: string
  relatedIssue?: number | undefined
  relatedPR?: number | undefined
}) {
  let output = `Store path: ${colorPath(msg.storePath)}

Run "pnpm install" to recreate node_modules.`

  if (msg.additionalInformation) {
    output = `${output}${EOL}${EOL}${msg.additionalInformation}`
  }

  output += formatRelatedSources(msg)

  return {
    title:
      'The store used for the current node_modules is incompatible with the current version of pnpm',
    body: output,
  }
}

function reportModulesBreakingChange(msg: {
  additionalInformation?: string | undefined
  modulesPath: string
  relatedIssue?: number | undefined
  relatedPR?: number | undefined
}): {
    title: string;
    body: string;
  } {
  let output = `node_modules path: ${colorPath(msg.modulesPath)}

Run ${highlight('pnpm install')} to recreate node_modules.`

  if (msg.additionalInformation) {
    output = `${output}${EOL}${EOL}${msg.additionalInformation}`
  }

  output += formatRelatedSources(msg)
  return {
    title:
      'The current version of pnpm is not compatible with the available node_modules structure',
    body: output,
  }
}

function formatRelatedSources(msg: {
  relatedIssue?: number | undefined
  relatedPR?: number | undefined
}): string {
  let output = ''

  if (!msg.relatedIssue && !msg.relatedPR) {
    return output
  }

  output += EOL

  if (msg.relatedIssue) {
    output +=
      EOL +
      `Related issue: ${colorPath(`https://github.com/pnpm/pnpm/issues/${msg.relatedIssue}`)}`
  }

  if (msg.relatedPR) {
    output +=
      EOL +
      `Related PR: ${colorPath(`https://github.com/pnpm/pnpm/pull/${msg.relatedPR}`)}`
  }

  return output
}

function formatGenericError(errorMessage: string, stack: object): {
  title: string;
  body: string;
} | {
  title: string;
} {
  if (stack) {
    let prettyStack: string | undefined

    try {
      prettyStack = new StackTracey(stack).asTable()
    } catch (err: unknown) {
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

function formatErrorSummary(message: string, code?: string | undefined): string {
  return `${chalk.bgRed.black(`\u2009${code ?? 'ERROR'}\u2009`)} ${chalk.red(message)}`
}

function reportModifiedDependency(msg: { modified: string[] }): {
  title: string;
  body: string;
} {
  return {
    title: 'Packages in the store have been mutated',
    body: `These packages are modified:
${msg.modified.map((pkgPath: string) => colorPath(pkgPath)).join(EOL)}

You can run ${highlight('pnpm install --force')} to refetch the modified packages`,
  }
}

function reportLockfileBreakingChange(err: Error, msg: object): {
  title: string;
  body: string;
} {
  return {
    title: err.message,
    body: `Run with the ${highlight('--force')} parameter to recreate the lockfile.`,
  }
}

function formatRecursiveCommandSummary(msg: {
  failures: Array<Error & { prefix: string }>
  passes: number
}): {
    title: string;
    body: string;
  } {
  const output =
    EOL +
    `Summary: ${chalk.red(`${msg.failures.length} fails`)}, ${msg.passes} passes` +
    EOL +
    EOL +
    msg.failures
      .map(({ message, prefix }) => {
        return prefix + ':' + EOL + formatErrorSummary(message)
      })
      .join(EOL + EOL)

  return {
    title: '',
    body: output,
  }
}

function reportBadTarballSize(err: Error, _msg: object): {
  title: string;
  body: string;
} {
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

function reportLifecycleError(msg: { stage: string; errno?: number | string }): {
  title: string;
} {
  if (msg.stage === 'test') {
    return { title: 'Test failed. See above for more details.' }
  }

  if (typeof msg.errno === 'number') {
    return { title: `Command failed with exit code ${msg.errno}.` }
  }

  return { title: 'Command failed.' }
}

function reportEngineError(msg: {
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
}): {
    title: string;
    body: string;
  } {
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

function reportAuthError(err: Error, msg: { hint?: string }, config?: Config) {
  const foundSettings: string[] = []

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

function hideSecureInfo(key: string, value: string): string {
  if (key.endsWith('_password')) {
    return '[hidden]'
  }

  if (key.endsWith('_auth') || key.endsWith('_authToken')) {
    return `${value.substring(0, 4)}[hidden]`
  }

  return value
}

function reportPeerDependencyIssuesError(
  err: Error,
  msg: { issuesByProjects: PeerDependencyIssuesByProjects },
  peerDependencyRules?: PeerDependencyRules
): {
  title: string;
  body: string;
} | null {
  const hasMissingPeers = getHasMissingPeers(msg.issuesByProjects)

  const hints: string[] = []

  if (hasMissingPeers) {
    hints.push(
      'If you want peer dependencies to be automatically installed, add "auto-install-peers=true" to an .npmrc file at the root of your project.'
    )
  }

  hints.push(
    'If you don\'t want pnpm to fail on peer dependency issues, add "strict-peer-dependencies=false" to an .npmrc file at the root of your project.'
  )

  const rendered = renderPeerIssues(msg.issuesByProjects, {
    rules: peerDependencyRules,
  })

  if (!rendered) {
    return null
  }

  return {
    title: err.message,
    body: `${rendered}
${hints.map((hint) => `hint: ${hint}`).join('\n')}
`,
  }
}

function getHasMissingPeers(issuesByProjects: PeerDependencyIssuesByProjects): boolean {
  return Object.values(issuesByProjects).some((issues): boolean => {
    return Object.values(issues.missing)
      .flat()
      .some(({ optional }): boolean => !optional);
  }
  )
}

function reportDedupeCheckIssuesError(
  err: Error,
  msg: { dedupeCheckIssues: DedupeCheckIssues }
): {
    title: string;
    body: string;
  } {
  return {
    title: err.message,
    body: `\
${renderDedupeCheckIssues(msg.dedupeCheckIssues)}
Run ${chalk.yellow('pnpm dedupe')} to apply the changes above.
`,
  }
}
