import * as lockfileMergeDriver from './lockfileMergeDriver'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import execa = require('execa')
import renderHelp = require('render-help')

const MERGE_DRIVER_CLI = require.resolve('npm-merge-driver/index.js')
const PNPM_MERGE_DRIVER_NAME = 'pnpm-lock-merge-driver'

export { lockfileMergeDriver }

export const installMergeDriver = {
  cliOptionsTypes: () => ({}),
  rcOptionsTypes: () => ({}),
  commandNames: ['install-merge-driver'],
  help: () => renderHelp({
    description: 'Set up the merge driver in the current Git repository.',
    usages: [],
  }),
  handler: async () => {
    execa.sync('node', [
      MERGE_DRIVER_CLI,
      'install',
      '--driver-name',
      PNPM_MERGE_DRIVER_NAME,
      '--driver',
      `pnpm ${lockfileMergeDriver.commandNames[0]} %A %O %B %P`,
      '--files',
      WANTED_LOCKFILE,
    ], { stdio: 'inherit' })
  },
}

export const uninstallMergeDriver = {
  cliOptionsTypes: () => ({}),
  rcOptionsTypes: () => ({}),
  commandNames: ['uninstall-merge-driver'],
  help: () => renderHelp({
    description: 'Remove a previously configured pnpm merge driver.',
    usages: [],
  }),
  handler: async () => {
    execa.sync('node', [
      MERGE_DRIVER_CLI,
      'uninstall',
      '--driver-name',
      PNPM_MERGE_DRIVER_NAME,
    ], { stdio: 'inherit' })
  },
}
