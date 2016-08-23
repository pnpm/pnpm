'use strict'
const writeFile = require('mz/fs').writeFile

module.exports = (path, json) => writeFile(path, JSON.stringify(json, null, 2) + '\n', 'utf8')
