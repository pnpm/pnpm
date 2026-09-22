# Adding TypeScript declarations

Use `pnpm add --save-types` to add registry packages and their available
DefinitelyTyped declarations in one command:

```sh
pnpm add express --save-types
pnpm add jest --save-dev --save-types
```

The first command saves `express` in `dependencies` and `@types/express` in
`devDependencies`. The second saves both packages in `devDependencies`.
Automatically added type packages always go in `devDependencies`, including
when the requested package is saved as an optional or peer dependency.

Packages that declare bundled types through `types`, `typings`, or a `types`
export condition do not need a companion package and are skipped. If the
registry returns 404 for the companion package, the requested package is still
added. Other registry errors are reported.

When a dependency's registry differs from the registry for `@types`, automatic
lookup is skipped unless an `@types:registry` is explicitly configured. This
prevents private scoped packages from silently pulling declarations from a
public registry. Explicitly adding a declaration package uses its normal
configured registry.

Scoped packages map to `@types/scope__name`. An npm alias gets a matching type
alias. Existing type dependencies for the same import name keep their declared ranges and groups;
explicit type package arguments take precedence over automatic additions.
New type packages use the latest version allowed by the configured resolution
policies, with the normal save prefix, exact-version, and catalog settings.
The declaration package's major version is not inferred from the runtime package.

The option applies to project dependencies, including filtered workspace adds
and `--lockfile-only`. It cannot be combined with `--global` or `--config`.
Automatic lookup applies to registry dependencies, including catalog entries
that reference registry packages. Local, workspace, Git, tarball, and JSR
sources do not trigger a DefinitelyTyped lookup.

To enable this behavior by default, set:

```yaml
saveTypes: true
```

in `pnpm-workspace.yaml`. Use `--no-save-types` to disable it for one invocation.
