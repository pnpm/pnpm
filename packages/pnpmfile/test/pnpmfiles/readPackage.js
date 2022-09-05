module.exports = {
  hooks: { readPackage }
}

function readPackage (pkg) {
  pkg.local = true
  return pkg
}
