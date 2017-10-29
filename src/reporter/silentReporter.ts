import {Log} from 'pnpm-logger'

export default (
  streamParser: {
    on: (event: 'data', handler: (obj: Log) => void) => void,
  },
) => {
  streamParser.on('data', (obj: Log) => {
    if (obj.level !== 'error') return

    console.log(obj['err'] && obj['err'].message || obj['message']) // tslint:disable-line
    if (obj['err'] && obj['err'] && obj['err'].stack) { // tslint:disable-line
      console.log(`\n${obj['err'].stack}`) // tslint:disable-line
    }
  })
}
