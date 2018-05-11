import fs = require('fs')
import gfs = require('graceful-fs')

gfs.gracefulify(fs)
