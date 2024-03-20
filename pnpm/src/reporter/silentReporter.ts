import type { LogBase } from '@pnpm/logger'
import { StreamParser } from '@pnpm/logger/lib/streamParser'

export function silentReporter(streamParser: StreamParser): void {
  streamParser.on('data', (obj: LogBase): void => {
    if (obj.level !== 'error') {
      return
    }

    // Known errors are not printed
    // @ts-ignore
    if (obj.err.code?.startsWith('ERR_PNPM_')) {
      return
    }

    // @ts-ignore
    console.log(obj.err?.message ?? obj.message)
    // @ts-ignore
    if (obj.err?.stack) {
      // @ts-ignore
      console.log(`\n${obj.err.stack}`)
    }
  })
}
