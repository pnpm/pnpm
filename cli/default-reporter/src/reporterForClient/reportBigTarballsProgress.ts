import { type FetchingProgressLog } from '@pnpm/core-loggers'
import type * as Rx from 'rxjs'
import { filter, map, startWith } from 'rxjs/operators'
import prettyBytes from 'pretty-bytes'
import {
  hlPkgId,
  hlValue,
} from './outputConstants'

const BIG_TARBALL_SIZE = 1024 * 1024 * 5 // 5 MB
const PRETTY_OPTS = {
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
}

export function reportBigTarballProgress (
  log$: {
    fetchingProgress: Rx.Observable<FetchingProgressLog>
  }
): Rx.Observable<Rx.Observable<{ fixed: boolean, msg: string }>> {
  return log$.fetchingProgress.pipe(
    filter((log: FetchingProgressLog) => log.status === 'started' &&
      typeof log.size === 'number' && log.size >= BIG_TARBALL_SIZE &&
      // When retrying the download, keep the existing progress line.
      // Fixing issue: https://github.com/pnpm/pnpm/issues/1013
      log.attempt === 1
    ),
    map((startedLog: FetchingProgressLog) => {
      const size = prettyBytes(startedLog.size ?? 0, PRETTY_OPTS)
      return log$.fetchingProgress.pipe(
        filter((log: FetchingProgressLog) => log.status === 'in_progress' && log.packageId === startedLog['packageId']),
        map((log: FetchingProgressLog) => log.downloaded ?? 0),
        startWith(0),
        map((downloadedRaw: number) => {
          const done = startedLog.size === downloadedRaw
          const downloaded = prettyBytes(downloadedRaw, PRETTY_OPTS)
          return {
            fixed: !done,
            msg: `Downloading ${hlPkgId(startedLog['packageId'])}: ${hlValue(downloaded)}/${hlValue(size)}${done ? ', done' : ''}`,
          }
        })
      )
    })
  )
}
