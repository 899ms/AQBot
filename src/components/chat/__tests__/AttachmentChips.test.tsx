import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  AttachmentChips,
  createComposerAttachment,
  revokeComposerAttachment,
  type ComposerAttachment,
} from '../AttachmentChips';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown> | string) => {
      if (typeof opts === 'string') return opts;
      if (key === 'chat.pastedTextLabel' && opts && typeof opts === 'object') {
        return `Pasted #${opts.n} (${opts.lines} lines)`;
      }
      return key;
    },
  }),
}));

describe('AttachmentChips', () => {
  let createObjectURL: ReturnType<typeof vi.fn>;
  let revokeObjectURL: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    createObjectURL = vi.fn((file: File) => `blob:preview-${file.name}`);
    revokeObjectURL = vi.fn();
    vi.stubGlobal('URL', {
      ...URL,
      createObjectURL,
      revokeObjectURL,
    });
  });

  it('creates a stable preview URL once and keeps the same img src across re-renders', () => {
    const file = new File([new Uint8Array([1, 2, 3])], 'photo.png', { type: 'image/png' });
    const attachment = createComposerAttachment(file, 'att-1');
    expect(createObjectURL).toHaveBeenCalledTimes(1);
    expect(attachment.previewUrl).toBe('blob:preview-photo.png');

    const onRemoveAttachment = vi.fn();
    const { rerender, container } = render(
      <AttachmentChips
        attachments={[attachment]}
        snippets={[]}
        onRemoveAttachment={onRemoveAttachment}
        onRemoveSnippet={vi.fn()}
      />,
    );

    const img = container.querySelector('img');
    expect(img).not.toBeNull();
    const srcBefore = img!.getAttribute('src');
    expect(srcBefore).toBe('blob:preview-photo.png');

    // Simulate parent re-render from typing (same attachment identity + callbacks).
    rerender(
      <AttachmentChips
        attachments={[attachment]}
        snippets={[]}
        onRemoveAttachment={onRemoveAttachment}
        onRemoveSnippet={vi.fn()}
      />,
    );

    const imgAfter = container.querySelector('img');
    expect(imgAfter?.getAttribute('src')).toBe(srcBefore);
    // createObjectURL must not be called again on re-render
    expect(createObjectURL).toHaveBeenCalledTimes(1);
  });

  it('revokes the preview URL when the attachment is removed', async () => {
    const user = userEvent.setup();
    const file = new File([new Uint8Array([1])], 'shot.jpg', { type: 'image/jpeg' });
    const attachment = createComposerAttachment(file, 'att-2');
    let attachments: ComposerAttachment[] = [attachment];
    const onRemoveAttachment = vi.fn((id: string) => {
      const target = attachments.find((item) => item.id === id);
      if (target) revokeComposerAttachment(target);
      attachments = attachments.filter((item) => item.id !== id);
    });

    const { rerender } = render(
      <AttachmentChips
        attachments={attachments}
        snippets={[]}
        onRemoveAttachment={onRemoveAttachment}
        onRemoveSnippet={vi.fn()}
      />,
    );

    await user.click(screen.getByLabelText('remove-attachment'));
    expect(onRemoveAttachment).toHaveBeenCalledWith('att-2');
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:preview-shot.jpg');

    rerender(
      <AttachmentChips
        attachments={attachments}
        snippets={[]}
        onRemoveAttachment={onRemoveAttachment}
        onRemoveSnippet={vi.fn()}
      />,
    );
    // after remove, parent re-renders with empty list → chips unmount
    expect(screen.queryByText('shot.jpg')).not.toBeInTheDocument();
  });
});
