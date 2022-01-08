module.exports = {
  hooks: {
    readPackage (pkg) {
      if (pkg.peerDependencies['eslint']) {
        pkg.peerDependencies['eslint'] = '*'
      }
      if (pkg.peerDependencies['@typescript-eslint/eslint-plugin'] === '^4.0.1') {
        pkg.peerDependencies['@typescript-eslint/eslint-plugin'] = '^5.6.0'
      }
      return pkg
    }
  }
}
