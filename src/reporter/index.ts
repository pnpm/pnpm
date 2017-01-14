import defaultReporter from 'pnpm-reporter-default'
import streamParser from '../logging/streamParser'
import bole = require('bole')

export type ReporterType = 'default' | 'ndjson'

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
    }
}
