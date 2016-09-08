import fs = require('mz/fs')

export default (path, json) => fs.writeFile(path, JSON.stringify(json, null, 2) + '\n', 'utf8')
