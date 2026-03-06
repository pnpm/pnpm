// Suppress "SQLite is an experimental feature" warnings without
// removing other warning listeners.
const originalEmit = process.emit.bind(process) as typeof process.emit
process.emit = function (event: string, ...args: unknown[]) {
  if (event === 'warning' && args[0] instanceof Error &&
      args[0].name === 'ExperimentalWarning' &&
      args[0].message.includes('SQLite')) {
    return false
  }
  return (originalEmit as Function).call(process, event, ...args) // eslint-disable-line @typescript-eslint/no-unsafe-function-type
} as typeof process.emit

import { startWorker } from './start.js'

startWorker()
