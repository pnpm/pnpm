require('bluebird').config({ warnings: false })
global.Promise = require('bluebird')
module.exports = global.Promise
