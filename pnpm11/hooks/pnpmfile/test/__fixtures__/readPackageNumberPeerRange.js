module.exports = {
  hooks: { readPackage },
}

function readPackage (pkg) {
  pkg.peerDependencies['is-positive'] = 1
  return pkg
}
