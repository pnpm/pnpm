module.exports = {
  hooks: { readPackage },
}

function readPackage (pkg) {
  pkg.optionalDependencies = false
  return pkg
}
