import { EventEmitter } from 'node:events'
import readline from 'node:readline'

import type { Log } from '@pnpm/core-loggers'
import type { Reporter, StreamParser } from '@pnpm/logger'

import { initDefaultReporter } from './index.js'

export async function runCli (argv: readonly string[]): Promise<void> {
  const cmd = argv[0] ?? 'install'

  const emitter = new EventEmitter()
  const streamParser: StreamParser<Log> = {
    on: (event: 'data', reporter: Reporter<Log>) => {
      emitter.on(event, reporter)
    },
    removeListener: (event: 'data', reporter: Reporter<Log>) => {
      emitter.removeListener(event, reporter)
    },
  }

  const close = initDefaultReporter({
    streamParser,
    context: { argv: [cmd] },
    reportingOptions: {
      throttleProgress: 200,
    },
  })

  // initDefaultReporter registers its 'data' listener via setTimeout(0); wait
  // a tick so events emitted from the readline loop below aren't dropped.
  await new Promise<void>((resolve) => { setTimeout(resolve, 0) })

  try {
    const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity })
    for await (const line of rl) {
      if (!line) continue
      let log: Log
      try {
        log = JSON.parse(line) as Log
      } catch {
        continue
      }
      emitter.emit('data', log)
    }
  } finally {
    close()
  }
}
