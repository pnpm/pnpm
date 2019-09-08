import { Log } from '@pnpm/core-loggers'
import PnpmError from '@pnpm/error'
import chalk from 'chalk'
import commonTags = require('common-tags')
import R = require('ramda')
import StackTracey = require('stacktracey')
import { EOL } from './constants'

StackTracey.maxColumnWidths = {
  callee: 25,
  file: 350,
  sourceLine: 25,
}

const stripIndent = commonTags.stripIndent
const stripIndents = commonTags.stripIndents
const highlight = chalk.yellow
const colorPath = chalk.gray

export default function reportError (logObj: Log) {
  if (logObj['err']) {
    const err = logObj['err'] as (PnpmError & { stack: object })
    switch (err.code) {
      case 'ERR_PNPM_UNEXPECTED_STORE':
        return reportUnexpectedStore(err, logObj['message'])
      case 'ERR_PNPM_STORE_BREAKING_CHANGE':
        return reportStoreBreakingChange(logObj['message'])
      case 'ERR_PNPM_MODULES_BREAKING_CHANGE':
        return reportModulesBreakingChange(logObj['message'])
      case 'ERR_PNPM_MODIFIED_DEPENDENCY':
        return reportModifiedDependency(logObj['message'])
      case 'ERR_PNPM_LOCKFILE_BREAKING_CHANGE':
        return reportLockfileBreakingChange(err, logObj['message'])
      case 'ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT':
        return formatErrorSummary(err.message)
      case 'ERR_PNPM_NO_MATCHING_VERSION':
        return formatNoMatchingVersion(err, logObj['message'])
      case 'ERR_PNPM_RECURSIVE_FAIL':
        return formatRecursiveCommandSummary(logObj['message'])
      case 'ERR_PNPM_BAD_TARBALL_SIZE':
        return reportBadTarballSize(err, logObj['message'])
      case 'ELIFECYCLE':
        return reportLifecycleError(logObj['message'])
      case 'ERR_PNPM_UNSUPPORTED_ENGINE':
        return reportEngineError(err, logObj['message'])
      default:
        // Errors with known error codes are printed w/o stack trace
        if (err.code && err.code.startsWith && err.code.startsWith('ERR_PNPM_')) {
          return formatErrorSummary(err.message)
        }
        return formatGenericError(err.message || logObj['message'], err.stack)
    }
  }
  return formatErrorSummary(logObj['message'])
}

function formatNoMatchingVersion (err: Error, msg: object) {
  const meta = msg['packageMeta']
  let output = stripIndent`
    ${formatErrorSummary(err.message)}

    The latest release of ${meta.name} is "${meta['dist-tags'].latest}".
  ` + EOL

  if (!R.equals(R.keys(meta['dist-tags']), ['latest'])) {
    output += EOL + 'Other releases are:' + EOL
    for (const tag in meta['dist-tags']) {
      if (tag !== 'latest') {
        output += `  * ${tag}: ${meta['dist-tags'][tag]}${EOL}`
      }
    }
  }

  output += `${EOL}If you need the full list of all ${Object.keys(meta.versions).length} published versions run "$ pnpm view ${meta.name} versions".`

  return output
}

function reportUnexpectedStore (err: Error, msg: object) {
  return stripIndent`
    ${formatErrorSummary(err.message)}

    The dependencies at "${msg['modulesDir']}" are currently linked from the store at "${msg['expectedStorePath']}".

    pnpm now wants to use the store at "${msg['actualStorePath']}" to link dependencies.

    If you want to use the new store location, reinstall your dependencies with "pnpm install --force".

    You may change the global store location by running "pnpm config set store <location>".
      (This error may happen if the node_modules was installed with a different major version of pnpm)
    `
}

function reportStoreBreakingChange (msg: object) {
  let output = stripIndent`
    ${formatErrorSummary(`The store used for the current node_modules is incomatible with the current version of pnpm`)}
    Store path: ${colorPath(msg['storePath'])}

    Try running the same command with the ${highlight('--force')} parameter.
  `

  if (msg['additionalInformation']) {
    output += EOL + EOL + msg['additionalInformation']
  }

  output += formatRelatedSources(msg)
  return output
}

function reportModulesBreakingChange (msg: object) {
  let output = stripIndent`
    ${formatErrorSummary(`The current version of pnpm is not compatible with the available node_modules structure`)}
    node_modules path: ${colorPath(msg['modulesPath'])}

    Run ${highlight('pnpm install --force')} to recreate node_modules.
  `

  if (msg['additionalInformation']) {
    output += EOL + EOL + msg['additionalInformation']
  }

  output += formatRelatedSources(msg)
  return output
}

function formatRelatedSources (msg: object) {
  let output = ''

  if (!msg['relatedIssue'] && !msg['relatedPR']) return output

  output += EOL

  if (msg['relatedIssue']) {
    output += EOL + `Related issue: ${colorPath(`https://github.com/pnpm/pnpm/issues/${msg['relatedIssue']}`)}`
  }

  if (msg['relatedPR']) {
    output += EOL + `Related PR: ${colorPath(`https://github.com/pnpm/pnpm/pull/${msg['relatedPR']}`)}`
  }

  return output
}

function formatGenericError (errorMessage: string, stack: object) {
  if (stack) {
    let prettyStack: string | undefined
    try {
      prettyStack = new StackTracey(stack).pretty
    } catch (err) {
      prettyStack = undefined
    }
    if (prettyStack) {
      return stripIndents`
          ${formatErrorSummary(errorMessage)}
          ${prettyStack}
        `
    }
  }
  return formatErrorSummary(errorMessage)
}

function formatErrorSummary (message: string) {
  return `${chalk.bgRed.black('\u2009ERROR\u2009')} ${chalk.red(message)}`
}

function reportModifiedDependency (msg: object) {
  return stripIndent`
    ${formatErrorSummary('Packages in the store have been mutated')}

    These packages are modified:
    ${msg['modified'].map((pkgPath: string) => colorPath(pkgPath)).join(EOL)}

    You can run ${highlight('pnpm install')} to refetch the modified packages
  `
}

function reportLockfileBreakingChange (err: Error, msg: object) {
  return stripIndent`
    ${formatErrorSummary(err.message)}

    Run with the ${highlight('--force')} parameter to recreate the lockfile.
  `
}

function formatRecursiveCommandSummary (msg: { fails: Array<Error & {prefix: string}>, passes: number }) {
  const output = EOL + `Summary: ${chalk.red(`${msg.fails.length} fails`)}, ${msg.passes} passes` + EOL + EOL +
    msg.fails.map((fail) => {
      return fail.prefix + ':' + EOL + formatErrorSummary(fail.message)
    }).join(EOL + EOL)
  return output
}

function reportBadTarballSize (err: Error, msg: object) {
  return stripIndent`
    ${formatErrorSummary(err.message)}

    Seems like you have internet connection issues.
    Try running the same command again.
    If that doesn't help, try one of the following:

    - Set a bigger value for the \`fetch-retries\` config.
        To check the current value of \`fetch-retries\`, run \`pnpm get fetch-retries\`.
        To set a new value, run \`pnpm set fetch-retries <number>\`.

    - Set \`network-concurrency\` to 1.
        This change will slow down installation times, so it is recommended to
        delete the config once the internet connection is good again: \`pnpm config delete network-concurrency\`

    NOTE: You may also override configs via flags.
    For instance, \`pnpm install --fetch-retries 5 --network-concurrency 1\`
  `
}

function reportLifecycleError (
  msg: {
    stage: string,
    errno?: number | string,
  },
) {
  if (msg.stage === 'test') {
    return formatErrorSummary('Test failed. See above for more details.')
  }
  if (typeof msg.errno === 'number') {
    return formatErrorSummary(`Command failed with exit code ${msg.errno}.`)
  }
  return formatErrorSummary('Command failed.')
}

function reportEngineError (
  err: Error,
  msg: {
    message: string,
    current: {
      node: string,
      pnpm: string,
    },
    packageId: string,
    wanted: {
      node?: string,
      pnpm?: string,
    },
  },
) {
  let output = ''
  if (msg.wanted.pnpm) {
    output += stripIndent`
      ${formatErrorSummary(`Your pnpm version is incompatible with "${msg.packageId}".`)}

      Expected version: ${msg.wanted.pnpm}
      Got: ${msg.current.pnpm}

      This is happening because the package's manifest has an engines.pnpm field specified.
      To fix this issue, install the required pnpm version globally.

      To install the latest version of pnpm, run "pnpm i -g pnpm".
      To check your pnpm version, run "pnpm -v".
    `
  }
  if (msg.wanted.node) {
    if (output) output += EOL + EOL
    output += stripIndent`
      ${formatErrorSummary(`Your Node version is incompatible with "${msg.packageId}".`)}

      Expected version: ${msg.wanted.node}
      Got: ${msg.current.node}

      This is happening because the package's manifest has an engines.node field specified.
      To fix this issue, install the required Node version.
    `
  }
  return output || formatErrorSummary(err.message)
}
