import bole = require('bole')
const logger = bole('progress')

export type ProgressLog = {
  pkg: LoggedPkg,
  status: 'resolving' | 'download-queued' | 'downloading' | 'download-start' | 'done' | 'dependencies' | 'error',
  downloadStatus?: DownloadStatus,
}

export type LoggedPkg = {
  rawSpec: string,
  name: string,
  version?: string,
}

export type DownloadStatus = {
  done: number,
  total: number,
}

export default (loginfo: ProgressLog) => logger.debug(loginfo)
