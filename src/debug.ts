import debug = require('debug')
import logger = require('@zkochan/logger')
const slice = Array.prototype.slice

const debugMap = {}

export default type => {
  debugMap[type] = debug(type)
  return logger.debug.bind(null, type)
}

logger.on('debug', function (ctx, level, type) {
  if (debugMap[type]) {
    const args = slice.call(arguments)
    debugMap[type].apply(debugMap[type], args.slice(3))
  }
})
