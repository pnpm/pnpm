import ndjson = require('ndjson')
import bole = require('bole')

export type StreamParser = {
  on: Function,
}

const streamParser: StreamParser = ndjson.parse()
bole.output([
  {
    level: 'debug', stream: streamParser
  },
])

export default streamParser
