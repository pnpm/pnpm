'use strict'
const reporter = require('../../../lib').default
const ndjson = require('ndjson')
const path = require('path')
const fs = require('fs')

const filename = path.join(__dirname, 'stdin')
const stream = fs.createReadStream(filename)
const streamParser = stream.pipe(ndjson.parse())
reporter(streamParser)

global.writeDebugLogFile = false
stream.on('close', () => process.exit(1))
