import defaultReporter from 'pnpm-default-reporter'
import silentReporter from './silentReporter'
import streamParser from '../logging/streamParser'
import bole = require('bole')

export type ReporterType = 'default' | 'ndjson' | 'silent'

export default (reporterType: ReporterType) => {
  switch (reporterType) {
    case 'default':
      defaultReporter(streamParser)
      return
    case 'ndjson':
      bole.output([
        {
          level: 'debug', stream: process.stdout
        },
      ])
      return
    case 'silent':
      silentReporter(streamParser)
      return
  }
}
