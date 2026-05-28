module.exports = {
  hooks: {
    readPackage (pkg) {
      if (pkg.name === '@pnpm/x') {
        if (!pkg.dependencies) {
          pkg.dependencies = {}
        }
        pkg.dependencies['@pnpm/y'] = '1.0.0'
      }
      return pkg
    }
  }
}
