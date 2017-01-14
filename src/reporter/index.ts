import defaultReporter from 'pnpm-reporter-default'
import streamParser from '../logging/streamParser'

export type ReporterType = 'default'

export default (reporterType: ReporterType) => defaultReporter(streamParser)
