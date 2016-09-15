import supportsColor = require('supports-color')
import pretty from './pretty'
import simple from './simple'

export type LoggerType = 'pretty' | 'simple'

export default (loggerType: LoggerType) =>
  ((loggerType === 'pretty' && supportsColor)
    ? pretty()
    : simple())
