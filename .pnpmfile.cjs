module.exports = {
  hooks: {
    readPackage (pkg) {
      if (typeof pkg.repository === 'string' && pkg.repository.startsWith('https://github.com/pnpm/pnpm/')) {
        pkg.devDependencies[pkg.name] = 'link:'
      }
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
