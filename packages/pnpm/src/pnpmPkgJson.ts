import loadJsonFile = require('load-json-file')
import path = require('path')

const pkgJson = loadJsonFile.sync(path.resolve(__dirname, '../package.json'))
const packageManager = {
  name: pkgJson.name,
  version: pkgJson.version,
}
export default packageManager
