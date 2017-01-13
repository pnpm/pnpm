import supportsColor = require('supports-color')
import pretty from './pretty'
import simple from './simple'

export type ReporterType = 'pretty' | 'simple'

export default (reporterType: ReporterType) =>
  ((reporterType === 'pretty' && supportsColor)
    ? pretty()
    : simple())
