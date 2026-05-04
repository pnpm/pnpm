import chalk from 'chalk'

export function formatWarn (message: string): string {
  return `${chalk.bgYellow.yellow('[')}${chalk.bgYellow.black('WARN')}${chalk.bgYellow.yellow(']')} ${message}`
}
