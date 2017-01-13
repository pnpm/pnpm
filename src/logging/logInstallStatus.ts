import bole = require('bole')
const logger = bole('progress')

export type LifecycleMessage = {
  pkgId: string,
  line: string,
}

export type ProgressMessage = {
  pkg: LoggedPkg,
  status: 'resolving' | 'download-queued' | 'downloading' | 'download-start' | 'done' | 'dependencies' | 'error',
  downloadStatus?: DownloadStatus,
}

export type InstallCheckMessage = {
  code: string,
  pkgid: string,
}

export type Log = {
  name: string,
  level: 'debug' | 'info' | 'warn' | 'error',
}

export type ProgressLog = Log & ProgressMessage

export type LifecycleLog = Log & LifecycleMessage

export type InstallCheckLog = Log & InstallCheckMessage

export type LoggedPkg = {
  rawSpec: string,
  name: string,
  version?: string,
}

export type DownloadStatus = {
  done: number,
  total: number,
}

export default (loginfo: ProgressMessage) => logger.debug(loginfo)
