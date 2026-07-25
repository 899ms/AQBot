import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();

vi.mock('@/lib/invoke', () => ({
  invoke: invokeMock,
}));

describe('providerStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('refetches providers after adding a key to a virtual builtin provider', async () => {
    const { useProviderStore } = await import('../providerStore');
    const materializedProviders = [
      {
        id: 'real-deepseek',
        builtin_id: 'deepseek',
        name: 'DeepSeek',
        keys: [{ id: 'key-1', provider_id: 'real-deepseek' }],
      },
    ];

    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'add_provider_key') {
        return { id: 'key-1', provider_id: 'real-deepseek' };
      }
      if (command === 'list_providers') {
        return materializedProviders;
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    useProviderStore.setState({
      providers: [
        {
          id: 'builtin_deepseek',
          builtin_id: 'deepseek',
          name: 'DeepSeek',
          keys: [],
        },
      ] as never,
      loading: false,
      error: null,
    });

    await useProviderStore.getState().addProviderKey('builtin_deepseek', 'sk-test');

    expect(invokeMock).toHaveBeenNthCalledWith(1, 'add_provider_key', {
      providerId: 'builtin_deepseek',
      rawKey: 'sk-test',
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'list_providers');
    expect(useProviderStore.getState().providers).toEqual(materializedProviders);
  });

  it('sends structured Bedrock credentials and stores the masked key record', async () => {
    const { useProviderStore } = await import('../providerStore');
    const key = {
      id: 'bedrock-key-1',
      provider_id: 'bedrock-1',
      key_prefix: 'AKIA1234…',
    };
    invokeMock.mockResolvedValue(key);
    useProviderStore.setState({
      providers: [{ id: 'bedrock-1', keys: [] }] as never,
      error: null,
    });
    const credentials = {
      access_key_id: 'AKIA123456789',
      secret_access_key: 'secret',
      session_token: 'session',
    };

    await useProviderStore.getState().addBedrockCredentials('bedrock-1', credentials);

    expect(invokeMock).toHaveBeenCalledWith('add_bedrock_credentials', {
      providerId: 'bedrock-1',
      credentials,
    });
    expect(useProviderStore.getState().providers[0].keys).toEqual([key]);
  });

  it('updates Bedrock credentials with the structured IPC command', async () => {
    const { useProviderStore } = await import('../providerStore');
    const updated = {
      id: 'bedrock-key-1',
      provider_id: 'bedrock-1',
      key_prefix: 'ASIA5678…',
    };
    invokeMock.mockResolvedValue(updated);
    useProviderStore.setState({
      providers: [{
        id: 'bedrock-1',
        keys: [{ id: 'bedrock-key-1', key_prefix: 'AKIA1234…' }],
      }] as never,
      error: null,
    });
    const credentials = {
      access_key_id: 'ASIA567890123',
      secret_access_key: 'new-secret',
      session_token: null,
    };

    await useProviderStore.getState().updateBedrockCredentials('bedrock-key-1', credentials);

    expect(invokeMock).toHaveBeenCalledWith('update_bedrock_credentials', {
      keyId: 'bedrock-key-1',
      credentials,
    });
    expect(useProviderStore.getState().providers[0].keys).toEqual([updated]);
  });

  it('refetches providers after saving models for a virtual builtin provider', async () => {
    const { useProviderStore } = await import('../providerStore');
    const models = [
      {
        provider_id: 'builtin_minimax',
        model_id: 'MiniMax-M1',
        name: 'MiniMax-M1',
        group_name: null,
        model_type: 'Chat',
        capabilities: ['TextChat'],
        context_window: 1000000,
        enabled: true,
        param_overrides: null,
      },
    ];
    const materializedProviders = [
      {
        id: 'real-minimax',
        builtin_id: 'minimax',
        name: 'MiniMax',
        models,
      },
    ];

    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'save_models') {
        return undefined;
      }
      if (command === 'list_providers') {
        return materializedProviders;
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    useProviderStore.setState({
      providers: [
        {
          id: 'builtin_minimax',
          builtin_id: 'minimax',
          name: 'MiniMax',
          models: [],
        },
      ] as never,
      loading: false,
      error: null,
    });

    await useProviderStore.getState().saveModels('builtin_minimax', models as never);

    expect(invokeMock).toHaveBeenNthCalledWith(1, 'save_models', {
      providerId: 'builtin_minimax',
      models,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'list_providers');
    expect(useProviderStore.getState().providers).toEqual(materializedProviders);
  });

  it('scans CC Switch provider import candidates', async () => {
    const { useProviderStore } = await import('../providerStore');
    const candidates = [
      {
        id: 'candidate-1',
        source_app: 'cc-switch',
        name: 'Claude Relay',
        provider_type: 'anthropic',
        api_host: 'https://api.anthropic.com',
        api_path: '/v1/messages',
        key_prefix: 'sk-ant...',
        models: ['claude-sonnet'],
        status: 'ready',
        reason: null,
      },
    ];

    invokeMock.mockResolvedValueOnce(candidates);

    await expect(useProviderStore.getState().scanCcSwitchProviderImports()).resolves.toEqual(candidates);
    expect(invokeMock).toHaveBeenCalledWith('scan_cc_switch_provider_imports');
  });

  it('imports selected CC Switch candidates and refreshes providers', async () => {
    const { useProviderStore } = await import('../providerStore');
    const result = {
      created_count: 1,
      added_key_count: 1,
      reused_count: 0,
      skipped_count: 0,
      provider_ids: ['provider-1'],
    };
    const providers = [
      {
        id: 'provider-1',
        builtin_id: null,
        name: 'Claude Relay',
      },
    ];

    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'import_cc_switch_provider_configs') {
        return result;
      }
      if (command === 'list_providers') {
        return providers;
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    await expect(
      useProviderStore.getState().importCcSwitchProviderConfigs(['candidate-1']),
    ).resolves.toEqual(result);

    expect(invokeMock).toHaveBeenNthCalledWith(1, 'import_cc_switch_provider_configs', {
      candidateIds: ['candidate-1'],
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'list_providers');
    expect(useProviderStore.getState().providers).toEqual(providers);
  });
});
