import loadJsonFile = require('load-json-file')
import {Package} from './types' // tslint:disable-line
import path = require('path')

export default loadJsonFile.sync(path.resolve(__dirname, '../package.json'))
