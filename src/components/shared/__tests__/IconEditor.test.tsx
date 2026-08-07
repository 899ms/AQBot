import { act, fireEvent, render, waitFor } from '@testing-library/react';
import { App, ConfigProvider } from 'antd';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  tauri: true,
  resolvedSrc: undefined as string | undefined,
}));

vi.mock('@/lib/invoke', () => ({
  invoke: mocks.invoke,
  isTauri: () => mocks.tauri,
}));

vi.mock('@/hooks/useResolvedAvatarSrc', () => ({
  useResolvedAvatarSrc: () => mocks.resolvedSrc,
}));

vi.mock('@/components/shared/EmojiPicker', () => ({
  EmojiPicker: () => null,
}));

vi.mock('@/components/shared/DynamicLobeIcon', () => ({
  DynamicLobeIcon: () => <div data-testid="lobe-icon" />,
}));

vi.mock('@/components/settings/IconPickerModal', () => ({
  default: () => null,
}));

// Avoid pulling emoji-picker-element / heavy icon graphs through relative imports.
vi.mock('../EmojiPicker', () => ({
  EmojiPicker: () => null,
}));

vi.mock('../DynamicLobeIcon', () => ({
  DynamicLobeIcon: () => <div data-testid="lobe-icon" />,
}));

import { IconEditor } from '../IconEditor';

function renderEditor(
  props: Partial<React.ComponentProps<typeof IconEditor>> & {
    onChange?: (type: string | null, value: string | null) => void;
  } = {},
) {
  const onChange = props.onChange ?? vi.fn();
  render(
    <ConfigProvider theme={{ token: { motion: false } }}>
      <App>
        <IconEditor
          iconType={props.iconType ?? null}
          iconValue={props.iconValue ?? null}
          onChange={onChange}
          size={40}
        />
      </App>
    </ConfigProvider>,
  );
  return { onChange };
}

describe('IconEditor file selection', () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
    mocks.tauri = true;
    mocks.resolvedSrc = undefined;
  });

  it('optimistically applies data URI then persists relative path', async () => {
    mocks.invoke.mockResolvedValueOnce('images/hash_avatar-1.png');
    const onChange = vi.fn();
    renderEditor({ onChange });

    const input = document.querySelector('input[type="file"]') as HTMLInputElement;
    expect(input).toBeTruthy();

    const file = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], 'a.png', {
      type: 'image/png',
    });
    const dataUri = 'data:image/png;base64,iVBORw0KGgo=';
    const original = FileReader.prototype.readAsDataURL;
    FileReader.prototype.readAsDataURL = function (this: FileReader) {
      Object.defineProperty(this, 'result', { value: dataUri, configurable: true });
      queueMicrotask(() => this.onload?.({} as ProgressEvent<FileReader>));
    };
    try {
      await act(async () => {
        fireEvent.change(input, { target: { files: [file] } });
      });
      await waitFor(() => {
        expect(onChange).toHaveBeenCalledWith('file', dataUri);
      });
      await waitFor(() => {
        expect(onChange).toHaveBeenCalledWith('file', 'images/hash_avatar-1.png');
      });
    } finally {
      FileReader.prototype.readAsDataURL = original;
    }

    expect(mocks.invoke).toHaveBeenCalledWith('save_avatar_file', {
      data: 'iVBORw0KGgo=',
      mimeType: 'image/png',
    });
  });

  it('does not use relative path as img src before resolve', () => {
    renderEditor({
      iconType: 'file',
      iconValue: 'images/hash_avatar-1.png',
    });
    const img = document.querySelector('img');
    if (img) {
      expect(img.getAttribute('src') ?? '').not.toContain('images/hash_avatar');
    }
  });

  it('renders direct data URI without waiting for resolve', () => {
    const dataUri = 'data:image/png;base64,iVBORw0KGgo=';
    renderEditor({ iconType: 'file', iconValue: dataUri });
    const img = document.querySelector('img');
    expect(img?.getAttribute('src')).toBe(dataUri);
  });
});
