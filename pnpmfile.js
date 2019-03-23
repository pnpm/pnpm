module.exports = {
  hooks: {
    readPackage (pkg) {
      if (!pkg.dependencies) return pkg
      if (pkg.dependencies['@commitlint/ensure']) {
        pkg.dependencies['@commitlint/ensure'] = '7.1.2'
      }
      if (pkg.name === '@types/p-any') {
        pkg.dependencies['@types/aggregate-error'] = '1.0.0'
      }
      return pkg
    },
  },
}
