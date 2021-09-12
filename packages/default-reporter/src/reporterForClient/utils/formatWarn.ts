import chalk from 'chalk'

export default function formatWarn (message: string) {
  return chalk.bold.yellow(`(!) ${message}`)
}
