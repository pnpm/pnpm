module.exports = {
  ...require('../../jest.config.js'),
  // This is a temporary workaround.
  // Currently, multiple tests use the @pnpm.e2e/foo package and they change it's dist-tags.
  // These tests are in separate files, so sometimes they will simultaneously set the dist tag and fail because they expect different versions to be tagged.
  maxWorkers: 1,
}
