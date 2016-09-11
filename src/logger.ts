import supportsColor = require('supports-color')
import pretty from './logger/pretty'
import simple from './logger/simple'

export type LoggerType = 'pretty' | 'simple'

export default (loggerType: LoggerType) =>
  ((loggerType === 'pretty' && supportsColor)
    ? pretty
    : simple())
