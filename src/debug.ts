import debug = require('debug')
import logger = require('@zkochan/logger')
const slice = Array.prototype.slice

const debugMap = {}

export default (type: string) => {
  debugMap[type] = debug(type)
  return logger.debug.bind(null, type)
}

logger.on('debug', function (ctx: any, level: string, type: string) {
  if (debugMap[type]) {
    const args = slice.call(arguments)
    debugMap[type].apply(debugMap[type], args.slice(3))
  }
})
