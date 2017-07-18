import fileReporter from 'pnpm-file-reporter'
import {streamParser} from 'pnpm-logger'

fileReporter(streamParser)
