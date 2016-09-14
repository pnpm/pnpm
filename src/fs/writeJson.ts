import fs = require('mz/fs')

export default (path: string, json: Object) => fs.writeFile(path, JSON.stringify(json, null, 2) + '\n', 'utf8')
