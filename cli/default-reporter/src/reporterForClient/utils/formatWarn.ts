import chalk from 'chalk'

export function formatWarn (message: string): string {
  return `${chalk.bgYellow.black('WARN:')} ${message}`
}
