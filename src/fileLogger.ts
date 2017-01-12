import fs = require('fs')
import YAML = require('json2yaml')
import streamParser from './logger/streamParser'

const slice = Array.prototype.slice
const logFilePath = 'pnpm-debug.log'
const logs: Object[] = []

streamParser.on('data', function (logObj: Object) {
  if (isUsefulLog(logObj)) {
    logs.push(logObj)
  }
})

function isUsefulLog (logObj: Object) {
  return logObj['name'] !== 'progress' || logObj['status'] !== 'downloading'
}

process.on('exit', (code: number) => {
  if (code === 0) {
    // it might not exist, so it is OK if it fails
    try {
      fs.unlinkSync(logFilePath)
    } catch (err) {}
    return
  }

  const yamlLogs = YAML.stringify(logs)
  fs.writeFileSync(logFilePath, yamlLogs, 'UTF8')
})
