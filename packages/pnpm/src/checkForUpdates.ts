import packageManager from '@pnpm/cli-meta'
import chalk = require('chalk')

export default async function () {
  let updateNotifier
  try {
    updateNotifier = (await import('update-notifier')).default
  } catch (err) {
    return
  }
  const notifier = updateNotifier({ pkg: packageManager })
  const update = notifier.update

  if (!update) {
    return
  }

  const message = `Update available! ${chalk.red(update.current)} → ${chalk.green(update.latest)}
${chalk.magenta('Changelog:')} https://github.com/pnpm/pnpm/releases/tag/v${update.latest}
Run ${chalk.magenta('pnpm i -g pnpm')} to update!

Follow ${chalk.magenta('@pnpmjs')} for updates: https://twitter.com/pnpmjs`

  notifier.notify({ message })
}
