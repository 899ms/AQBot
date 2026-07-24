import { describe, expect, it } from 'vitest';

const manifestModule =
  // @ts-expect-error The workflow helper is native ESM without TypeScript declarations.
  await import('../../../scripts/generate-updater-manifest.mjs');
const { buildUpdaterManifest, classifySignature } = manifestModule;

function assetPair(name: string, id: number) {
  return [
    { id, name },
    { id: id + 1, name: `${name}.sig` },
  ];
}

const assets = [
  ...assetPair('AQBot_0.0.102_amd64.AppImage', 1),
  ...assetPair('AQBot_0.0.102_amd64.deb', 3),
  ...assetPair('AQBot-0.0.102-1.x86_64.rpm', 5),
  ...assetPair('AQBot_aarch64.app.tar.gz', 7),
  ...assetPair('AQBot_x64.app.tar.gz', 9),
  ...assetPair('AQBot_0.0.102_arm64_en-US.msi', 11),
  ...assetPair('AQBot_0.0.102_arm64-setup.exe', 13),
  ...assetPair('AQBot_0.0.102_x64_en-US.msi', 15),
  ...assetPair('AQBot_0.0.102_x64-setup.exe', 17),
];

const signatures = new Map(
  assets
    .filter(({ name }) => name.endsWith('.sig'))
    .map(({ name }) => [name, `signature:${name}`]),
);

describe('updater manifest generation', () => {
  it('classifies supported updater signature assets', () => {
    expect(classifySignature('AQBot_aarch64.app.tar.gz.sig')).toEqual({
      os: 'darwin',
      arch: 'aarch64',
      bundle: 'app',
      primary: true,
    });
    expect(classifySignature('AQBot_0.0.102_x64-setup.exe.sig')).toEqual({
      os: 'windows',
      arch: 'x86_64',
      bundle: 'nsis',
      primary: false,
    });
    expect(classifySignature('AQBot_0.0.102_x64-portable.zip.sig')).toBeNull();
  });

  it('builds one complete updater manifest after all platform uploads', () => {
    const manifest = buildUpdaterManifest({
      version: '0.0.102',
      notes: 'release notes',
      pubDate: '2026-07-24T15:00:00.000Z',
      repository: 'AQBot-Desktop/AQBot',
      serverUrl: 'https://github.com',
      tag: 'v0.0.102',
      assets,
      signatures,
    });

    expect(manifest.version).toBe('0.0.102');
    expect(manifest.notes).toBe('release notes');
    expect(manifest.platforms['windows-aarch64'].url).toBe(
      'https://github.com/AQBot-Desktop/AQBot/releases/download/v0.0.102/AQBot_0.0.102_arm64_en-US.msi',
    );
    expect(manifest.platforms['windows-aarch64-nsis'].signature).toBe(
      'signature:AQBot_0.0.102_arm64-setup.exe.sig',
    );
    expect(manifest.platforms['darwin-x86_64-app'].url).toBe(
      'https://github.com/AQBot-Desktop/AQBot/releases/download/v0.0.102/AQBot_x64.app.tar.gz',
    );
    expect(manifest.platforms['linux-x86_64-rpm'].url).toBe(
      'https://github.com/AQBot-Desktop/AQBot/releases/download/v0.0.102/AQBot-0.0.102-1.x86_64.rpm',
    );
    expect(Object.keys(manifest.platforms)).toHaveLength(14);
  });

  it('fails instead of publishing an incomplete updater manifest', () => {
    expect(() =>
      buildUpdaterManifest({
        version: '0.0.102',
        notes: '',
        pubDate: '2026-07-24T15:00:00.000Z',
        repository: 'AQBot-Desktop/AQBot',
        serverUrl: 'https://github.com',
        tag: 'v0.0.102',
        assets: assets.filter(
          ({ name }) => !name.includes('AQBot_x64.app.tar.gz'),
        ),
        signatures,
      }),
    ).toThrow('missing updater platforms: darwin-x86_64, darwin-x86_64-app');
  });
});
