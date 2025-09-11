---
"@pnpm/plugin-commands-installation": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/store-connection-manager": minor
"@pnpm/npm-resolver": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

There have been several incidents recently where popular packages were successfully attacked. To reduce the risk of installing a compromised version, we are introducing a new setting that delays the installation of newly released dependencies. In most cases, such attacks are discovered quickly and the malicious versions are removed from the registry within an hour.

The new setting is called `minimumReleaseAge`. It specifies the number of minutes that must pass after a version is published before pnpm will install it. For example, setting `minimumReleaseAge: 1440` ensures that only packages released at least one day ago can be installed.

If you set `minimumReleaseAge` but need to disable this restriction for certain dependencies, you can list them under the `minimumReleaseAgeExclude` setting. For instance, with the following configuration pnpm will always install the latest version of webpack, regardless of its release time:

```yaml
minimumReleaseAgeExclude:
- webpack
```

Related issue: [#9921](https://github.com/pnpm/pnpm/issues/9921).
