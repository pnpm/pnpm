module.exports = {
  hooks: {
    readPackage (pkg) {
      switch (pkg.name) {
      case '@babel/parser':
        pkg.peerDependencies['@babel/types'] = '*'
        break
      }
      return pkg
    }
  }
}
