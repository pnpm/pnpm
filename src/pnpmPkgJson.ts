import requireJson from './fs/requireJson'
import {Package} from './api/initCmd' // tslint:disable-line
import path = require('path')

export default requireJson(path.resolve(__dirname, '../package.json'))
