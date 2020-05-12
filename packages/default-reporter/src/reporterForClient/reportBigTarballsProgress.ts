import { FetchingProgressLog } from '@pnpm/core-loggers'
import most = require('most')
import prettyBytes = require('pretty-bytes')
import {
  hlPkgId,
  hlValue,
} from './outputConstants'

const BIG_TARBALL_SIZE = 1024 * 1024 * 5 // 5 MB

export default (
  log$: {
    fetchingProgress: most.Stream<FetchingProgressLog>,
  }
) => {
  return log$.fetchingProgress
    .filter((log) => log.status === 'started' &&
      typeof log.size === 'number' && log.size >= BIG_TARBALL_SIZE &&
      // When retrying the download, keep the existing progress line.
      // Fixing issue: https://github.com/pnpm/pnpm/issues/1013
      log.attempt === 1
    )
    .map((startedLog) => {
      const size = prettyBytes(startedLog['size'])
      return log$.fetchingProgress
        .filter((log) => log.status === 'in_progress' && log.packageId === startedLog['packageId'])
        .map((log) => log['downloaded'])
        .startWith(0)
        .map((downloadedRaw) => {
          const done = startedLog['size'] === downloadedRaw
          const downloaded = prettyBytes(downloadedRaw)
          return {
            fixed: !done,
            msg: `Downloading ${hlPkgId(startedLog['packageId'])}: ${hlValue(downloaded)}/${hlValue(size)}${done ? ', done' : ''}`,
          }
        })
    })
}
