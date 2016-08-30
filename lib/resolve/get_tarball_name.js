'use strict'
const basename = require('path').basename

module.exports = tarballPath => basename(tarballPath).replace(/(\.tgz|\.tar\.gz)$/i, '')
