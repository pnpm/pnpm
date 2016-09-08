import supportsColor = require('supports-color')
import pretty from './logger/pretty'
import simple from './logger/simple'

export default loggerType =>
  ((loggerType === 'pretty' && supportsColor)
    ? pretty
    : simple())
