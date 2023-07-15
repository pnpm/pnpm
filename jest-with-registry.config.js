const path = require("path");

module.exports = {
  ...require('./jest.config'),
  globalSetup: path.join(__dirname, 'jest.globalSetup.js'),
  globalTeardown: path.join(__dirname, 'jest.globalTeardown.js'),
}
