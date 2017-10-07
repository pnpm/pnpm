# @pnpm/self-installer

> Installs and uninstalls pnpm

## Usage

Use [unpkg](https://unpkg.com/) to access the installation script and set up pnpm:

```
curl -L https://unpkg.com/@pnpm/self-installer | node
```

The above script will install the latest version of pnpm but you may also install
a specific version:

```
curl -L https://unpkg.com/@pnpm/self-installer@1.16.2 | node
```

You can also use a semver range:

```
curl -L https://unpkg.com/@pnpm/self-installer@1 | node
```

Or a tag:

```
curl -L https://unpkg.com/@pnpm/self-installer@next | node
```

**NOTE:** Installation by version or range works from `1.16.2` only.

## License

[MIT](https://github.com/pnpm/pnpm/blob/master/.scripts/self-installer/LICENSE)
