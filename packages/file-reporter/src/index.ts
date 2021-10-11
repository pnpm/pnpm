import path from 'path'
import fs from 'graceful-fs'

const LOG_FILENAME = 'node_modules/.pnpm-debug.log'

export default function (streamParser: Object) {
  const logs: Object[] = []

  streamParser['on']('data', function (logObj: Object) {
    if (isUsefulLog(logObj)) {
      logs.push(logObj)
    }
  })

  process.on('exit', (code: number) => {
    if (code === 0) {
      // it might not exist, so it is OK if it fails
      try {
        fs.unlinkSync(LOG_FILENAME)
      } catch (err) {}
      return
    }

    const prettyLogs = getPrettyLogs()
    const jsonLogs = JSON.stringify(prettyLogs, null, 2)
    // Don't create a node_modules directory if it does not exist
    const dest = fs.existsSync(path.dirname(LOG_FILENAME))
      ? LOG_FILENAME
      : path.basename(LOG_FILENAME)
    fs.writeFileSync(dest, jsonLogs, 'utf-8')
  })

  function getPrettyLogs () {
    const prettyLogs = {}
    logs.forEach((logObj, i) => {
      // eslint-disable-next-line
      const key = `${i} ${logObj['level']} ${logObj['name']}`
      const msgobj = getMessageObj(logObj)
      prettyLogs[key] = prettify(msgobj)
    })
    return prettyLogs
  }

  function getMessageObj (logobj: Object): Object {
    const msgobj = {}
    for (const key in logobj) {
      if (['time', 'hostname', 'pid', 'level', 'name'].includes(key)) continue
      msgobj[key] = logobj[key]
    }
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
