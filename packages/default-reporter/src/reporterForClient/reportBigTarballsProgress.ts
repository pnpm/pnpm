import { FetchingProgressLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { filter, map, startWith } from 'rxjs/operators'
import {
  hlPkgId,
  hlValue,
} from './outputConstants'
import prettyBytes = require('pretty-bytes')

const BIG_TARBALL_SIZE = 1024 * 1024 * 5 // 5 MB

export default (
  log$: {
    fetchingProgress: Rx.Observable<FetchingProgressLog>
  }
) => {
  return log$.fetchingProgress.pipe(
    filter((log: FetchingProgressLog) => log.status === 'started' &&
      typeof log.size === 'number' && log.size >= BIG_TARBALL_SIZE &&
      // When retrying the download, keep the existing progress line.
      // Fixing issue: https://github.com/pnpm/pnpm/issues/1013
      log.attempt === 1
    ),
    map((startedLog: FetchingProgressLog) => {
      const size = prettyBytes(startedLog['size'])
      return log$.fetchingProgress.pipe(
        filter((log: FetchingProgressLog) => log.status === 'in_progress' && log.packageId === startedLog['packageId']),
        map((log: FetchingProgressLog) => log['downloaded']),
        startWith(0),
        map((downloadedRaw: number) => {
          const done = startedLog['size'] === downloadedRaw
          const downloaded = prettyBytes(downloadedRaw)
          return {
            fixed: !done,
            msg: `Downloading ${hlPkgId(startedLog['packageId'])}: ${hlValue(downloaded)}/${hlValue(size)}${done ? ', done' : ''}`,
          }
        })
      )
    })
  )
}
