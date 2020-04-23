module.exports = {
  hooks: {
    readPackage (pkg) {
      if (pkg.dependencies['@nodelib/fs.walk'] === '^1.1.0') {
        pkg.dependencies['@nodelib/fs.walk'] = '1.1.1'
      }
      if (pkg.dependencies['istanbul-reports']) {
        pkg.dependencies['istanbul-reports'] = 'npm:@zkochan/istanbul-reports'
      }
      if (pkg.name === 'verdaccio') {
        pkg.dependencies['http-errors'] = '^1.7.3'
      }
      return pkg
    }
  }
}
