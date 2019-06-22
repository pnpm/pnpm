module.exports = {
  hooks: {
    readPackage (pkg) {
      if (pkg.dependencies['@nodelib/fs.walk'] === '^1.1.0') {
        pkg.dependencies['@nodelib/fs.walk'] = '1.1.1'
      }
      return pkg
    }
  }
}
