import bole from 'bole'

export function writeToConsole (): void {
  bole.output([
    {
      level: 'debug', stream: process.stdout,
    },
  ])
}
