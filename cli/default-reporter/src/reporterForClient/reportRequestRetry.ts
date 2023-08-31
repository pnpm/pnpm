import { type RequestRetryLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'
import prettyMilliseconds from 'pretty-ms'
import { formatWarn } from './utils/formatWarn'

export function reportRequestRetry (
  requestRetry$: Rx.Observable<RequestRetryLog>
) {
  return requestRetry$.pipe(
    map((log) => {
      const retriesLeft = log.maxRetries - log.attempt + 1
      const errorCode = log.error['httpStatusCode'] || log.error['status'] || log.error['errno'] || log.error['code']
      const msg = `${log.method} ${log.url} error (${errorCode}). \
Will retry in ${prettyMilliseconds(log.timeout, { verbose: true })}. \
${retriesLeft} retries left.`
      return Rx.of({ msg: formatWarn(msg) })
    })
  )
}
