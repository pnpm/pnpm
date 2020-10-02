const config = require('../../jest.config.js')
module.exports = Object.assign({}, config, {
  // Shallow so fixtures aren't matched
  testMatch: ["**/test/*.[jt]s?(x)"]
})