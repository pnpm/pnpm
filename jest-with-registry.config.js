const path = require("path");

module.exports = {
  ...require('./jest.config'),
  // Many tests change the dist tags of packages.
  // Unfortunately, this means that if two such tests will run at the same time,
  // they may break each other.
  maxWorkers: 1,
  globalSetup: path.join(__dirname, 'jest.globalSetup.js'),
  globalTeardown: path.join(__dirname, 'jest.globalTeardown.js'),
}
