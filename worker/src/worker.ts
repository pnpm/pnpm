// We don't want to see "SQLite is an experimental feature and might change at any time" warnings
process.removeAllListeners('warning').on('warning', err => {
  if (err.name !== 'ExperimentalWarning' && !err.message.includes('experimental')) {
    console.warn(err)
  }
})

import { startWorker } from './start.js'

startWorker()
