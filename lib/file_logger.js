'use strict'
const logger = require('@zkochan/logger')
const fs = require('fs')
const YAML = require('json2yaml')
const slice = Array.prototype.slice

const logFilePath = 'pnpm-debug.log'

const logs = []

logger.onAny(function () {
  const args = slice.call(arguments).slice(2)
  if (isUsefulLog.apply(null, args)) {
    logs.push(args)
  }
})

function isUsefulLog (level) {
  return level !== 'progress' || arguments[2] !== 'downloading'
}

process.on('exit', code => {
  if (code === 0) {
    fs.unlink(logFilePath)
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

function stringify (obj) {
  if (obj instanceof Error) {
    let logMsg = obj.toString()
    if (obj.stack) {
      logMsg += `\n${obj.stack}`
    }
    return logMsg
  }
  return obj
}

function mergeStrings (arr) {
  const mergedArr = []
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
