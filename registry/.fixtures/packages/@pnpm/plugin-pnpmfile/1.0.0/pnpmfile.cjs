module.exports = {
  hooks: {
    updateConfig: (config) => ({
      ...config,
      nodeLinker: 'hoisted',
    }),
  }
}
