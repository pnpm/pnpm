module.exports = {
  hooks: {
    readPackage (pkg) {
      if (!pkg.dependencies) return pkg
      if (pkg.dependencies['@commitlint/ensure']) {
        pkg.dependencies['@commitlint/ensure'] = '7.1.2'
      }
      return pkg
    },
  },
}
