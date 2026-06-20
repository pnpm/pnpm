module.exports = {
  hooks: {
    readPackage: (pkg) => {
      pkg._fromCjs = true
      return pkg
    },
  }
}
