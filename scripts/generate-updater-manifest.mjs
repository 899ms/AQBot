#!/usr/bin/env node
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const REQUIRED_PLATFORMS = [
  'darwin-aarch64',
  'darwin-aarch64-app',
  'darwin-x86_64',
  'darwin-x86_64-app',
  'linux-x86_64',
  'linux-x86_64-appimage',
  'linux-x86_64-deb',
  'linux-x86_64-rpm',
  'windows-aarch64',
  'windows-aarch64-msi',
  'windows-aarch64-nsis',
  'windows-x86_64',
  'windows-x86_64-msi',
  'windows-x86_64-nsis',
];

export function classifySignature(name) {
  const mac = name.match(/_(aarch64|x64)\.app\.tar\.gz\.sig$/);
  if (mac) {
    return {
      os: 'darwin',
      arch: mac[1] === 'x64' ? 'x86_64' : 'aarch64',
      bundle: 'app',
      primary: true,
    };
  }

  const windows = name.match(/_(arm64|x64)(?:_[^/]+\.msi|-setup\.exe)\.sig$/);
  if (windows) {
    const bundle = name.endsWith('.msi.sig') ? 'msi' : 'nsis';
    return {
      os: 'windows',
      arch: windows[1] === 'x64' ? 'x86_64' : 'aarch64',
      bundle,
      primary: bundle === 'msi',
    };
  }

  if (/_amd64\.AppImage\.sig$/.test(name)) {
    return { os: 'linux', arch: 'x86_64', bundle: 'appimage', primary: true };
  }
  if (/_amd64\.deb\.sig$/.test(name)) {
    return { os: 'linux', arch: 'x86_64', bundle: 'deb', primary: false };
  }
  if (/\.x86_64\.rpm\.sig$/.test(name)) {
    return { os: 'linux', arch: 'x86_64', bundle: 'rpm', primary: false };
  }
  return null;
}

function releaseAssetUrl(serverUrl, repository, tag, assetName) {
  return [
    serverUrl.replace(/\/$/, ''),
    repository,
    'releases',
    'download',
    encodeURIComponent(tag),
    encodeURIComponent(assetName),
  ].join('/');
}

function addPlatform(platforms, key, value) {
  if (platforms[key]) {
    throw new Error(`duplicate updater platform: ${key}`);
  }
  platforms[key] = value;
}

export function buildUpdaterManifest(options) {
  const assetNames = new Set(options.assets.map(({ name }) => name));
  const platforms = {};

  for (const asset of [...options.assets].sort((a, b) => a.name.localeCompare(b.name))) {
    const target = classifySignature(asset.name);
    if (!target) continue;

    const installerName = asset.name.slice(0, -'.sig'.length);
    if (!assetNames.has(installerName)) {
      throw new Error(`updater installer missing for signature: ${asset.name}`);
    }
    const signature = options.signatures.get(asset.name);
    if (!signature) {
      throw new Error(`updater signature content missing: ${asset.name}`);
    }
    const value = {
      signature,
      url: releaseAssetUrl(
        options.serverUrl,
        options.repository,
        options.tag,
        installerName,
      ),
    };
    const baseKey = `${target.os}-${target.arch}`;
    if (target.primary) addPlatform(platforms, baseKey, value);
    addPlatform(platforms, `${baseKey}-${target.bundle}`, value);
  }

  const missing = REQUIRED_PLATFORMS.filter((key) => !platforms[key]);
  if (missing.length > 0) {
    throw new Error(`missing updater platforms: ${missing.join(', ')}`);
  }
  return {
    version: options.version,
    notes: options.notes,
    pub_date: options.pubDate,
    platforms,
  };
}

function githubHeaders(token, accept) {
  return {
    accept,
    authorization: `Bearer ${token}`,
    'user-agent': 'aqbot-release-workflow',
    'x-github-api-version': '2022-11-28',
  };
}

async function githubRequest(url, token, options = {}) {
  const {
    accept = 'application/vnd.github+json',
    headers = {},
    ...requestOptions
  } = options;
  const response = await fetch(url, {
    ...requestOptions,
    headers: {
      ...githubHeaders(token, accept),
      ...headers,
    },
  });
  if (!response.ok) {
    const details = await response.text();
    throw new Error(
      `${requestOptions.method ?? 'GET'} ${url} failed: ${response.status} ${details}`,
    );
  }
  return response;
}

async function listPages(apiUrl, path, token) {
  const values = [];
  for (let page = 1; ; page += 1) {
    const separator = path.includes('?') ? '&' : '?';
    const response = await githubRequest(
      `${apiUrl}${path}${separator}per_page=100&page=${page}`,
      token,
    );
    const pageValues = await response.json();
    values.push(...pageValues);
    if (pageValues.length < 100) return values;
  }
}

async function findDraftRelease(apiUrl, repository, tag, token) {
  const releases = await listPages(apiUrl, `/repos/${repository}/releases`, token);
  const release = releases.find((value) => value.tag_name === tag && value.draft);
  if (!release) {
    throw new Error(`draft release not found for tag ${tag}`);
  }
  return release;
}

async function downloadSignatures(apiUrl, repository, assets, token) {
  const signatureAssets = assets.filter(({ name }) => classifySignature(name));
  const entries = await Promise.all(
    signatureAssets.map(async (asset) => {
      const response = await githubRequest(
        `${apiUrl}/repos/${repository}/releases/assets/${asset.id}`,
        token,
        { accept: 'application/octet-stream' },
      );
      return [asset.name, await response.text()];
    }),
  );
  return new Map(entries);
}

async function replaceManifest(release, existingAsset, contents, apiUrl, repository, token) {
  if (existingAsset) {
    await githubRequest(
      `${apiUrl}/repos/${repository}/releases/assets/${existingAsset.id}`,
      token,
      { method: 'DELETE' },
    );
  }
  const uploadUrl = release.upload_url.replace(/\{\?name,label\}$/, '');
  await githubRequest(`${uploadUrl}?name=latest.json`, token, {
    method: 'POST',
    accept: 'application/vnd.github+json',
    headers: {
      'content-type': 'application/json',
      'content-length': Buffer.byteLength(contents).toString(),
    },
    body: contents,
  });
}

async function main() {
  const token = process.env.GITHUB_TOKEN;
  const repository = process.env.GITHUB_REPOSITORY;
  const tag = process.env.GITHUB_REF_NAME;
  if (!token || !repository || !tag) {
    throw new Error('GITHUB_TOKEN, GITHUB_REPOSITORY and GITHUB_REF_NAME are required');
  }

  const apiUrl = (process.env.GITHUB_API_URL ?? 'https://api.github.com').replace(/\/$/, '');
  const serverUrl = process.env.GITHUB_SERVER_URL ?? 'https://github.com';
  const release = await findDraftRelease(apiUrl, repository, tag, token);
  const assets = await listPages(
    apiUrl,
    `/repos/${repository}/releases/${release.id}/assets`,
    token,
  );
  const signatures = await downloadSignatures(apiUrl, repository, assets, token);
  const manifest = buildUpdaterManifest({
    version: tag.replace(/^v/, ''),
    notes: release.body ?? '',
    pubDate: new Date().toISOString(),
    repository,
    serverUrl,
    tag,
    assets,
    signatures,
  });
  const contents = `${JSON.stringify(manifest, null, 2)}\n`;
  const existingAsset = assets.find(({ name }) => name === 'latest.json');
  await replaceManifest(release, existingAsset, contents, apiUrl, repository, token);
  console.log(`Uploaded latest.json with ${Object.keys(manifest.platforms).length} platforms.`);
}

const entry = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : '';
if (entry === import.meta.url) {
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
