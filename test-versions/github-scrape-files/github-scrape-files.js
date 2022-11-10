#! /usr/bin/env node

import fs from 'fs'
import path from 'path'

import { githubToken } from './secret.js'

import { Octokit } from "@octokit/rest";
import fetch from 'node-fetch'

const query = 'filename:pnpm-lock.yaml';

// v3
// only way to code search
const octokit = new Octokit({
  auth: githubToken,
});

const sleep = ms => new Promise(r => setTimeout(r, ms));

// Only the first 1000 search results are available
// -> per_page = 10 && page_max = 100
// -> per_page = 100 && page_max = 10

//const page_max = 100;
const page_max = 10;
for (var page = 1; page <= page_max; page++) {

  console.log(`page = ${page}`)

  // https://octokit.github.io/rest.js/v18#search
  const result = await (async () => {
    try {
      return await octokit.rest.search.code({
        q: query,

        // indexed: indicates how recently a file has been indexed by the GitHub search infrastructure
        // Default: best match
        sort: 'indexed',
        order: 'asc',
        //order: 'desc',

        per_page: 100, // max 100

        page,
        //page: 1,
      });
    }
    catch (error) {
      console.log('got error:')
      console.log(error.message);
      return error.response;
    }
  })();

  console.log(`x-ratelimit-used: ${result.headers['x-ratelimit-used']} of ${result.headers['x-ratelimit-limit']}`)

  //console.log(result);

  const data = result.data;

  if (data.message) {
    console.log(data.message);
    page--;
    console.log(`sleep 60 seconds`)
    await sleep(60 * 1000);
    continue;
  }

  if (data.items) {

    console.log(`found ${data.total_count} files`)

    console.log(`downloading ${data.items.length} files`)

    for (const item of data.items) {
      const [owner, repo] = item.repository.full_name.split("/");
      const commit = item.url.split('=').pop();
      const filePath = `github/${owner}/${repo}/${commit}/${item.path}`;
      if (fs.existsSync(filePath) == false) {
        console.log(`exists ${filePath}`)
      }
      else {
        const url = `https://github.com/${owner}/${repo}/raw/${commit}/${item.path}`;
        // example:
        // https://api.github.com/repositories/477274833/contents/pnpm-lock.yaml?ref=a720de682b6b0f42c37973aba1dd11a58eb0423f
        const fileResponse = await fetch(url)
        const fileText = await fileResponse.text();
        fs.mkdirSync(path.dirname(filePath), { recursive: true });
        fs.writeFileSync(filePath, fileText, "utf8");
        console.log(`done ${filePath}`)
      }
    }
  }

  else {
    console.log('rate limit exceeded?');
  }

  console.log(`sleep 60 seconds`)
  await sleep(60 * 1000);

}
