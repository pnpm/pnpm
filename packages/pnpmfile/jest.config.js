const config = require('../../jest.config.js');

module.exports = Object.assign({}, config, {
  // we ignore test/pnpmfiles helpers
  testMatch: ["**/test/*.[jt]s?(x)"],
});
