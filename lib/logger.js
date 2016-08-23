'use strict'
const supportsColor = require('supports-color')

module.exports = loggerType =>
  ((loggerType === 'pretty' && supportsColor)
    ? require('./logger/pretty')
    : require('./logger/simple'))()
