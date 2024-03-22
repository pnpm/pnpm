import type * as Rx from 'rxjs'
import prettyBytes from 'pretty-bytes'
import { filter, map, startWith } from 'rxjs/operators'

import type { FetchingProgressLog } from '@pnpm/types'

import { hlPkgId, hlValue } from './outputConstants.js'

const BIG_TARBALL_SIZE = 1024 * 1024 * 5 // 5 MB
const PRETTY_OPTS = {
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
}

export function reportBigTarballProgress(log$: {
  fetchingProgress: Rx.Observable<FetchingProgressLog>
}): Rx.Observable<Rx.Observable<{
    fixed: boolean;
    msg: string;
  }>> {
  return log$.fetchingProgress.pipe(
    filter(
      (log: FetchingProgressLog) =>
        log.status === 'started' &&
        typeof log.size === 'number' &&
        log.size >= BIG_TARBALL_SIZE &&
        // When retrying the download, keep the existing progress line.
        // Fixing issue: https://github.com/pnpm/pnpm/issues/1013
        log.attempt === 1
    ),
    map((startedLog: FetchingProgressLog): Rx.Observable<{
      fixed: boolean;
      msg: string;
    }> => {
      const size =
        'size' in startedLog && typeof startedLog.size === 'number'
          ? prettyBytes(startedLog.size, PRETTY_OPTS)
          : '0 kB'

      return log$.fetchingProgress.pipe(
        filter(
          (log: FetchingProgressLog): boolean => {
            return log.status === 'in_progress' &&
              log.packageId === startedLog.packageId;
          }
        ),
        map((log: FetchingProgressLog): number => {
          return 'downloaded' in log && typeof log.downloaded === 'number'
            ? log.downloaded
            : 0;
        }),
        startWith(0),
        map(
          (
            downloadedRaw: number
          ): {
            fixed: boolean
            msg: string
          } => {
            const done =
              'size' in startedLog &&
              typeof startedLog.size === 'number' &&
              startedLog.size === downloadedRaw

            const downloaded = prettyBytes(downloadedRaw, PRETTY_OPTS)

            return {
              fixed: !done,
              msg: `Downloading ${hlPkgId(startedLog.packageId)}: ${hlValue(downloaded)}/${hlValue(size)}${done ? ', done' : ''}`,
            }
          }
        )
      )
    })
  )
}
