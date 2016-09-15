import logger = require('@zkochan/logger')
import fs = require('fs')
import YAML = require('json2yaml')
const slice = Array.prototype.slice

const logFilePath = 'pnpm-debug.log'

const logs: Object[][] = []

logger.onAny(function () {
  const args = slice.call(arguments).slice(2)
  if (isUsefulLog.apply(null, args)) {
    logs.push(args)
  }
})

function isUsefulLog (level: string) {
  return level !== 'progress' || arguments[2] !== 'downloading'
}

process.on('exit', (code: number) => {
  if (code === 0) {
    // it might not exist, so it is OK if it fails
    try {
      fs.unlinkSync(logFilePath)
    } catch (err) {}
    return
  }

  const prettyLogs = getPrettyLogs()
  const yamlLogs = YAML.stringify(prettyLogs)
  fs.writeFileSync(logFilePath, yamlLogs, 'UTF8')
})

function getPrettyLogs () {
  const logObj = {}
  logs.forEach((args, i) => {
    const key = `${i} ${args[0]} ${args[1]}`
    const rest = mergeStrings(args.slice(2).map(stringify))
    logObj[key] = rest.length === 1 ? rest[0] : rest
  })
  return logObj
}

function stringify (obj: Object): string {
  if (obj instanceof Error) {
    let logMsg = obj.toString()
    if (obj.stack) {
      logMsg += `\n${obj.stack}`
    }
    return logMsg
  }
  return obj.toString()
}

function mergeStrings (arr: Object[]) {
  const mergedArr: Object[] = []
  let prevWasString = false
  arr.forEach(el => {
    const currentIsString = typeof el === 'string'
    if (currentIsString && prevWasString) {
      mergedArr[mergedArr.length - 1] += ' ' + el
    } else {
      mergedArr.push(el)
    }
    prevWasString = currentIsString
  })
  return mergedArr
}
