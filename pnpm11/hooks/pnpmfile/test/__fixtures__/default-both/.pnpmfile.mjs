export const hooks = {
  readPackage: (pkg) => {
    pkg._fromMjs = true
    return pkg
  },
}
