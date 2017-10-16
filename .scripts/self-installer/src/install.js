'use strict'
const path = require('path')
const installTo = require('./installTo')

const dest = path.join(process.execPath, '../../lib/node_modules')
const binPath = path.dirname(process.execPath)

installTo(dest, binPath)
  .catch(err => {
    console.error(err)
    process.exit(1)
  })
