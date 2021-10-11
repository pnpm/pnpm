'use strict'
const reporter = require('../../../lib').default
const ndjson = require('ndjson')
const path = require('path')
const fs = require('fs')

const filename = path.join(__dirname, 'stdin')
const stream = fs.createReadStream(filename)
const streamParser = stream.pipe(ndjson.parse())
reporter(streamParser)

stream.on('close', () => process.exit(0))
