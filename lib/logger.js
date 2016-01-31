var config = require('./config')
var supportsColor = require('supports-color')

module.exports =
  (config.logger === 'pretty' && supportsColor)
    ? require('./logger/pretty')
    : require('./logger/simple')
