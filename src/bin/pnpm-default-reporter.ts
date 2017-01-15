#!/usr/bin/env node
import ndjson = require('ndjson')
import reporter from '..'

process.stdin.resume()
process.stdin.setEncoding('utf8')
const streamParser = process.stdin
  .pipe(ndjson.parse())
reporter(streamParser)
