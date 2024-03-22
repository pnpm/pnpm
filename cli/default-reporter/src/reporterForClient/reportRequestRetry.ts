import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'
import prettyMilliseconds from 'pretty-ms'

import type { RequestRetryLog } from '@pnpm/types'

import { formatWarn } from './utils/formatWarn.js'

export function reportRequestRetry(
  requestRetry$: Rx.Observable<RequestRetryLog>
): Rx.Observable<Rx.Observable<{
    msg: string;
  }>> {
  return requestRetry$.pipe(
    map((log: RequestRetryLog): Rx.Observable<{
      msg: string;
    }> => {
      const retriesLeft = log.maxRetries - log.attempt + 1

      const errorCode =
      // @ts-ignore
        log.error.httpStatusCode ||
        // @ts-ignore
        log.error.status ||
        // @ts-ignore
        log.error.errno ||
        // @ts-ignore
        log.error.code

      const msg = `${log.method} ${log.url} error (${errorCode}). \
Will retry in ${prettyMilliseconds(log.timeout, { verbose: true })}. \
${retriesLeft} retries left.`

      return Rx.of({ msg: formatWarn(msg) })
    })
  )
}
