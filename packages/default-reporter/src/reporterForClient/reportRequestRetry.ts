import { RequestRetryLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'
import formatWarn from './utils/formatWarn'
import prettyMilliseconds = require('pretty-ms')

export default (
  requestRetry$: Rx.Observable<RequestRetryLog>
) => {
  return requestRetry$.pipe(
    map((log) => {
      const retriesLeft = log.maxRetries - log.attempt + 1
      const errorCode = log.error['httpStatusCode'] || log.error['status'] || log.error['errno'] || log.error['code']
      // eslint-disable-next-line @typescript-eslint/restrict-template-expressions
      const msg = `${log.method} ${log.url} error (${errorCode}). \
Will retry in ${prettyMilliseconds(log.timeout, { verbose: true })}. \
${retriesLeft} retries left.`
      return Rx.of({ msg: formatWarn(msg) })
    })
  )
}
