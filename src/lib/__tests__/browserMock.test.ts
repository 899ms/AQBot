import { beforeEach, describe, expect, it } from 'vitest';

import { handleCommand } from '../browserMock';

type GatewayTemplate = {
  id: string;
  target: string;
  content: string;
};

const EXPECTED_SHUAI_API_PROVIDER = {
  id: 'builtin-shuaiapi',
  builtin_id: 'shuaiapi',
  name: 'SHUAI API',
  provider_type: 'openai',
  api_host: 'https://api.shuaiapi.com',
  api_path: null,
  enabled: false,
  models: [],
  keys: [],
  proxy_config: null,
  sort_order: 9,
  created_at: 1700000000000,
  updated_at: 1700000000000,
};

describe('browserMock built-in providers', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('initializes SHUAI API with the expected fields and ordering', async () => {
    const providers = await handleCommand<any[]>('list_providers');
    const shuaiApi = providers.find((provider) => provider.id === 'builtin-shuaiapi');

    expect(shuaiApi).toEqual(EXPECTED_SHUAI_API_PROVIDER);
    expect(providers.map((provider) => provider.id)).toEqual(expect.arrayContaining([
      'builtin-minimax',
      'builtin-shuaiapi',
      'builtin-jina',
    ]));
    expect(providers.findIndex((provider) => provider.id === 'builtin-shuaiapi')).toBe(
      providers.findIndex((provider) => provider.id === 'builtin-minimax') + 1,
    );
    expect(providers.findIndex((provider) => provider.id === 'builtin-jina')).toBe(
      providers.findIndex((provider) => provider.id === 'builtin-shuaiapi') + 1,
    );
    expect(providers.find((provider) => provider.id === 'builtin-jina')?.sort_order).toBe(10);
    expect(providers.find((provider) => provider.id === 'builtin-cohere')?.sort_order).toBe(11);
    expect(providers.find((provider) => provider.id === 'builtin-voyage')?.sort_order).toBe(12);
  });

  it('adds the complete SHUAI API provider to existing localStorage', async () => {
    const providers = await handleCommand<any[]>('list_providers');
    const legacySortOrders: Record<string, number> = {
      'builtin-jina': 9,
      'builtin-cohere': 10,
      'builtin-voyage': 11,
    };
    const legacyProviders = providers
      .filter((provider) => provider.id !== 'builtin-shuaiapi')
      .map((provider) => ({
        ...provider,
        sort_order: legacySortOrders[provider.id] ?? provider.sort_order,
      }));
    localStorage.setItem(
      'aqbot_providers',
      JSON.stringify(legacyProviders),
    );

    const upgradedProviders = await handleCommand<any[]>('list_providers');
    const shuaiApi = upgradedProviders.find((provider) => provider.id === 'builtin-shuaiapi');
    const persistedProviders = JSON.parse(localStorage.getItem('aqbot_providers') ?? '[]');

    expect(shuaiApi).toEqual(EXPECTED_SHUAI_API_PROVIDER);
    expect(persistedProviders.find((provider: any) => provider.id === 'builtin-shuaiapi'))
      .toEqual(EXPECTED_SHUAI_API_PROVIDER);
    expect(upgradedProviders.findIndex((provider) => provider.id === 'builtin-shuaiapi')).toBe(
      upgradedProviders.findIndex((provider) => provider.id === 'builtin-minimax') + 1,
    );
    expect(upgradedProviders.find((provider) => provider.id === 'builtin-jina')?.sort_order).toBe(10);
    expect(upgradedProviders.find((provider) => provider.id === 'builtin-cohere')?.sort_order).toBe(11);
    expect(upgradedProviders.find((provider) => provider.id === 'builtin-voyage')?.sort_order).toBe(12);
  });

  it('does not share mutable built-in provider data across initializations', async () => {
    const providers = await handleCommand<any[]>('list_providers');
    const shuaiApi = providers.find((provider) => provider.id === 'builtin-shuaiapi');
    shuaiApi.keys.push({ id: 'temporary-key' });

    localStorage.clear();
    const reinitializedProviders = await handleCommand<any[]>('list_providers');

    expect(reinitializedProviders.find((provider) => provider.id === 'builtin-shuaiapi'))
      .toEqual(EXPECTED_SHUAI_API_PROVIDER);
  });
});

describe('browserMock gateway templates', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('returns Claude and Cursor templates that match AQBot runtime contracts', async () => {
    const templates = await handleCommand<GatewayTemplate[]>('list_gateway_templates');

    const cursor = templates.find((template) => template.target === 'cursor');
    expect(cursor).toBeDefined();
    expect(cursor?.content).toContain('"openai.apiKey"');
    expect(cursor?.content).toContain('"openai.apiBaseUrl"');
    expect(cursor?.content).not.toContain('"api_key"');
    expect(cursor?.content).not.toContain('"api_base"');

    const claude = templates.find((template) => template.target === 'claude_code');
    expect(claude).toBeDefined();
    expect(claude?.content).toContain('ANTHROPIC_BASE_URL=');
    expect(claude?.content).toContain('ANTHROPIC_AUTH_TOKEN=');
    expect(claude?.content).not.toContain('ANTHROPIC_API_KEY=');
  });

  it('maps backup manifests into files-page backup rows and cleans up missing entries', async () => {
    await handleCommand('create_backup', { format: 'sqlite' });

    const rows = await handleCommand<any[]>('list_files_page_entries', { category: 'backups' });
    expect(rows).toHaveLength(1);
    expect(rows[0].id).toMatch(/^backup_manifest::/);
    expect(rows[0].category).toBe('backups');
    expect(rows[0].path).toContain('/mock/path/');

    await handleCommand('cleanup_missing_files_page_entry', { entryId: rows[0].id });

    const backups = await handleCommand<any[]>('list_backups');
    expect(backups).toHaveLength(0);
  });

  it('exposes raw stored-file ids for files-page image protocol URLs', async () => {
    localStorage.setItem('aqbot_drawing_files', JSON.stringify([{
      id: 'stored-image-1',
      original_name: 'preview.png',
      mime_type: 'image/png',
      size_bytes: 68,
      storage_path: 'images/preview.png',
      data: 'ignored-by-files-page-list',
    }]));

    const rows = await handleCommand<any[]>('list_files_page_entries', { category: 'images' });

    expect(rows).toEqual([
      expect.objectContaining({
        id: 'attachment::stored-image-1',
        storedFileId: 'stored-image-1',
        storagePath: 'images/preview.png',
      }),
    ]);
  });

  it('stores S3 config and supports S3 backup list/delete commands', async () => {
    await handleCommand('save_s3_config', {
      config: {
        bucket: 'aqbot-backups',
        region: 'us-west-2',
        prefix: 'desktop/',
        endpointUrl: null,
        forcePathStyle: false,
        useDefaultCredentials: false,
        accessKeyId: 'access',
        secretAccessKey: 'secret',
        sessionToken: null,
      },
    });

    const config = await handleCommand<any>('get_s3_config');
    expect(config.bucket).toBe('aqbot-backups');

    const fileName = await handleCommand<string>('s3_backup');
    const backups = await handleCommand<any[]>('s3_list_backups');
    expect(backups[0].fileName).toBe(fileName);

    await handleCommand('s3_delete_backup', { fileName });
    const remaining = await handleCommand<any[]>('s3_list_backups');
    expect(remaining).toHaveLength(0);
  });

  it('flattens MCP create input and updates only input fields', async () => {
    const created = await handleCommand<any>('create_mcp_server', {
      input: {
        name: 'Remote MCP',
        transport: 'http',
        endpoint: 'https://example.com/mcp',
        headersJson: JSON.stringify({ Authorization: 'Bearer old' }),
        enabled: false,
      },
    });

    expect(created.name).toBe('Remote MCP');
    expect(created.transport).toBe('http');
    expect(created.endpoint).toBe('https://example.com/mcp');
    expect(created.headersJson).toBe(JSON.stringify({ Authorization: 'Bearer old' }));
    expect(created.input).toBeUndefined();

    const updated = await handleCommand<any>('update_mcp_server', {
      id: created.id,
      input: {
        headersJson: JSON.stringify({ Authorization: 'Bearer new' }),
      },
    });

    expect(updated.id).toBe(created.id);
    expect(updated.headersJson).toBe(JSON.stringify({ Authorization: 'Bearer new' }));
    expect(updated.input).toBeUndefined();
  });
});
