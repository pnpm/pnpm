import chalk = require('chalk')
import {Log} from 'pnpm-logger'

export default function reportError (logObj: Log) {
  if (logObj['err']) {
    const err = <Error & { code: string }>logObj['err']
    switch (err.code) {
      case 'UNEXPECTED_STORE':
        reportUnexpectedStore(err, logObj['message'])
        return
      case 'STORE_BREAKING_CHANGE':
        reportStoreBreakingChange(err, logObj['message'])
        return
      case 'MODULES_BREAKING_CHANGE':
        reportModulesBreakingChange(err, logObj['message'])
        return
      default:
        printErrorSummary(err.message || logObj['message'])
        return
    }
  }
  printErrorSummary(logObj['message'])
}

function reportUnexpectedStore (err: Error, msg: Object) {
  printErrorSummary(err.message)
  console.log()
  console.log(`expected: ${chalk.yellow(msg['expectedStorePath'])}`)
  console.log(`actual: ${chalk.yellow(msg['actualStorePath'])}`)
  console.log()
  console.log(`If you want to use the new store, run the same command with the ${chalk.yellow('--force')} parameter.`)
}

function reportStoreBreakingChange (err: Error, msg: Object) {
  printErrorSummary(`The store used for the current node_modules is incomatible with the current version of pnpm`)
  console.log(`Store path: ${chalk.gray(msg['storePath'])}`)
  console.log()
  console.log(`Try running the same command with the ${chalk.yellow('--force')} parameter.`)
  if (msg['additionalInformation']) {
    console.log()
    console.log(msg['additionalInformation'])
  }
  printRelatedSources(msg)
}

function reportModulesBreakingChange (err: Error, msg: Object) {
  printErrorSummary(`The current version of pnpm is not compatible with the available node_modules structure`)
  console.log(`node_modules path: ${chalk.gray(msg['modulesPath'])}`)
  console.log()
  console.log(`Try running the same command with the ${chalk.yellow('--force')} parameter.`)
  if (msg['additionalInformation']) {
    console.log()
    console.log(msg['additionalInformation'])
  }
  printRelatedSources(msg)
}

function printRelatedSources (msg: Object) {
  if (!msg['relatedIssue'] && !msg['relatedPR']) return
  console.log()
  if (msg['relatedIssue']) {
    console.log(`Related issue: ${chalk.gray(`https://github.com/pnpm/pnpm/issues/${msg['relatedIssue']}`)}`)
  }
  if (msg['relatedPR']) {
    console.log(`Related PR: ${chalk.gray(`https://github.com/pnpm/pnpm/pull/${msg['relatedPR']}`)}`)
  }
}

function printErrorSummary (message: string) {
  console.log(chalk.red('ERROR'), message)
}
