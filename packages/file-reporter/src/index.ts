import path from 'path'
import fs from 'graceful-fs'

const LOG_FILENAME = 'node_modules/.pnpm-debug.log'

export default function (streamParser: Object) {
  let logNum = 0

  // Clean up previous log files
  if (global['writeDebugLogFile'] !== false) {
    if (fs.existsSync(LOG_FILENAME)) fs.unlinkSync(LOG_FILENAME)
    if (fs.existsSync(path.basename(LOG_FILENAME))) fs.unlinkSync(path.basename(LOG_FILENAME))
  }

  streamParser['on']('data', function (logObj: Object) {
    if (isUsefulLog(logObj) && global['writeDebugLogFile'] !== false) {
      const msgobj = getMessageObj(logObj)
      const prettyLogs = prettify(msgobj)
      const jsonLogs = JSON.stringify(prettyLogs, null, 2) + '\n'
      const dest = fs.existsSync(path.dirname(LOG_FILENAME)) ? LOG_FILENAME : path.basename(LOG_FILENAME)
      fs.appendFileSync(dest, jsonLogs)
      logNum++
    }
  })

  function getMessageObj (logobj: Object): Object {
    const msgobj: { [key: string]: string } = {}
    for (const key in logobj) {
      if (['time', 'hostname', 'pid', 'level', 'name'].includes(key)) continue
      msgobj[key] = logobj[key]
    }
    const logLevel: string = logobj['level']
    const logName: string = logobj['name']
    msgobj.key = `${logNum} ${logLevel} ${logName}`
    return msgobj
  }

  function prettify (obj: Object): string | Object {
    if (obj instanceof Error) {
      let logMsg = obj.toString()
      if (obj.stack) {
        logMsg += `\n${obj.stack}`
      }
      return logMsg
    }
    if (Object.keys(obj).length === 1 && obj['message']) return obj['message']
    return obj
  }
}

function isUsefulLog (logObj: Object) {
  return logObj['name'] !== 'pnpm:progress' || logObj['status'] !== 'downloading'
}
