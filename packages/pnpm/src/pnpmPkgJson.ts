import loadJsonFile = require('load-json-file')
import path = require('path')

export default loadJsonFile.sync(path.resolve(__dirname, '../package.json'))
