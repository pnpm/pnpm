'use strict'
module.exports = pkg => pkg.name.replace('/', '!') + '@' + pkg.version
