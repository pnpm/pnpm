var config = require('./config')

module.exports =
  config.pnpm_logger === 'pretty'
    ? require('./logger/pretty')
    : require('./logger/simple')
