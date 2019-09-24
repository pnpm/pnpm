module.exports = {
  hooks: {
    readPackage (pkg) {
      if (pkg.dependencies['@nodelib/fs.walk'] === '^1.1.0') {
        pkg.dependencies['@nodelib/fs.walk'] = '1.1.1'
      }
      if (pkg.name === 'verdaccio') {
        pkg.dependencies['http-errors'] = '^1.7.3'
      }
      return pkg
    }
  }
}
