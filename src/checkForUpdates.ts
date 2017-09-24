import updateNotifier = require('update-notifier')
import chalk = require('chalk')
import pkg from './pnpmPkgJson'
import {stripIndents} from 'common-tags'

export default function () {
  const notifier = updateNotifier({ pkg })
  const update = notifier.update

  if (!update) {
    return
  }

  const message = stripIndents`
    Update available! ${chalk.red(update.current)} → ${chalk.green(update.latest)}
    ${chalk.magenta('Changelog:')} https://github.com/pnpm/pnpm/releases/tag/v${update.latest}
    Run ${chalk.magenta('npm i -g pnpm')} to update!

    Follow ${chalk.magenta('@pnpmjs')} for updates: https://twitter.com/pnpmjs
  `

  notifier.notify({ message })
}
