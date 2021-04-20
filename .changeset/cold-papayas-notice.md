---
"@pnpm/filter-workspace-packages": major
---

# @pnpm/filter-workspace-packages

Change `@pnpm/filter-workspace-packages` to handle the new `filter-prod` flag, so that devDependencies are ignored if the filters / packageSelectors include `followProdDepsOnly` as true. 

## filterPackages

WHAT: Change `filterPackages`'s second arg to accept an array of objects with properties `filter` and `followProdDepsOnly`.

WHY: Allow `filterPackages` to handle the filter-prod flag which allows the omission of devDependencies when building the package graph.

HOW: Update your code by converting the filters into an array of objects. The `filter` property of this object maps to the filter that was previously passed in. The `followProdDepsOnly` is a boolean that will
ignore devDependencies when building the package graph.

If you do not care about ignoring devDependencies and want `filterPackages` to work as it did in the previous major version then you can use a simple map to convert your filters.

```
const newFilters = oldFilters.map(filter => ({ filter, followProdDepsOnly: false }));
```
