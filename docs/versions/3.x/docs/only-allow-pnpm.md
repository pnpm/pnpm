---
id: only-allow-pnpm
title: Only allow pnpm
original_id: only-allow-pnpm
---

When you use pnpm on a project, you don't want others to accidentally run `npm install` or `yarn`.
To prevent devs from using other package managers, add the following `preinstall` script to your `package.json`:

```json
{
  "scripts": {
    "preinstall": "node -e '!process.env.npm_config_user_agent.startsWith(\"pnpm/\")&&!console.log(\"Use `npx pnpm install` to install dependencies in this repository\\n\")&&process.exit(1)'"
  }
}
```

Next time, when someone runs `npm install` or `yarn install`, they'll get an error message in the console.
