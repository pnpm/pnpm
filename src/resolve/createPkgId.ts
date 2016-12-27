import path = require('path')
import {Package} from '../types'

export const delimiter = '+'

export default (pkg: Package): string => path.join(pkg.name, pkg.version)
