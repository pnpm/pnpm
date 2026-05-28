const path = require('path')

module.exports = {
  hooks: {
    updateConfig (config) {
      config.patchedDependencies = {
        ...config.patchedDependencies,
        '@pnpm.e2e/foo': path.join(__dirname, '@pnpm.e2e__foo@100.0.0.patch'),
      }
      return config
    },
  },
}
