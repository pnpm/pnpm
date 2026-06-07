module.exports = {
  hooks: {
    updateConfig (config) {
      config.catalogs ??= {}
      config.catalogs.default ??= {}
      config.catalogs.default['@pnpm.e2e/foo'] = '100.0.0'
      config.catalogs.bar ??= {}
      config.catalogs.bar['@pnpm.e2e/bar'] = '100.0.0'
      return config
    },
  },
}
