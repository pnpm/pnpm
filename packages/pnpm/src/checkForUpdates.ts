import packageManager from '@pnpm/cli-meta'
import chalk = require('chalk')
import { stripIndents } from 'common-tags'
import updateNotifier = require('update-notifier')

export default function () {
  const notifier = updateNotifier({ pkg: packageManager })
  const update = notifier.update

  if (!update) {
    return
  }

  // Although, "pnpm i -g pnpm" works in most cases, we have a big amount of users
  // experiencing issues with it: https://github.com/pnpm/pnpm/issues/1203
  // So until the issues are not fixed, we are recommending to update pnpm using npm.
  const message = stripIndents`
    Update available! ${chalk.red(update.current)} → ${chalk.green(update.latest)}
    ${chalk.magenta('Changelog:')} https://github.com/pnpm/pnpm/releases/tag/v${update.latest}
    Run ${chalk.magenta('npm i -g pnpm')} to update!

    Follow ${chalk.magenta('@pnpmjs')} for updates: https://twitter.com/pnpmjs
  `

  notifier.notify({ message })
}
