const config = require('../../jest.config')

module.exports = {
  ...config,
  modulePathIgnorePatterns: ['\/fixtures\/.*'],
}
