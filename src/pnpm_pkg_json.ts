import requireJson from './fs/require_json'
import {Package} from './api/init_cmd' // tslint:disable-line
import path = require('path')

export default requireJson(path.resolve(__dirname, '../package.json'))
