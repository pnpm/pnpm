---
id: only-allow-pnpm
title: Only allow pnpm
---

When you use pnpm on a project, you don't want others to accidentally run
`npm install` or `yarn`. To prevent devs from using other package managers,
you can add the following `preinstall` script to your `package.json`:

```json
{
	"scripts": {
		"preinstall": "npx only-allow pnpm"
	}
}
```

Now, whenever someone runs `npm install` or `yarn`, they'll get an
error instead and installation will not proceed.

If you use npm v7, use `npx -y` instead.
