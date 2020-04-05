import { LogBase } from '@pnpm/logger'

export default (
  streamParser: {
    on: (event: 'data', handler: (obj: LogBase) => void) => void,
  },
) => {
  streamParser.on('data', (obj: LogBase) => {
    if (obj.level !== 'error') return

    // Known errors are not printed
    if (obj['err'].code?.startsWith('ERR_PNPM_')) return

    console.log(obj['err']?.message ?? obj['message']) // tslint:disable-line
    if (obj['err']?.stack) { // tslint:disable-line
      console.log(`\n${obj['err'].stack}`) // tslint:disable-line
    }
  })
}
