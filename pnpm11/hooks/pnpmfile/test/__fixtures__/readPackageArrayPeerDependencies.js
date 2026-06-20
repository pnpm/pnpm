module.exports = {
  hooks: { readPackage },
}

function readPackage (pkg) {
  pkg.peerDependencies = []
  return pkg
}
