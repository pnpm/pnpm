import {Log} from 'pnpm-logger'

export default (streamParser: {on: Function}) => {
  streamParser.on('data', (obj: Log) => {
    if (obj.level !== 'error') return

    console.log(obj['err'] && obj['err'].message || obj['message'])
    if (obj['err'] && obj['err'] && obj['err'].stack) {
      console.log(`\n${obj['err'].stack}`)
    }
  })
}
