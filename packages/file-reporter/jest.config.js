const config = require('../../jest.config.js');

module.exports = {
  ...config,
  testMatch: ["**/test/index.ts"],
}
