import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { describe, expect, it } from 'vitest';
import { AcpThreadSelectCheckbox } from '../AcpThreadSelectCheckbox';

function SelectableRow() {
  const [selected, setSelected] = useState(false);
  return (
    <li data-testid="row" onClick={() => setSelected((value) => !value)}>
      <AcpThreadSelectCheckbox checked={selected} onCheckedChange={setSelected} />
      <span>{selected ? 'on' : 'off'}</span>
    </li>
  );
}

function paintedCheckbox(container: HTMLElement): HTMLElement {
  const wrapper = container.querySelector<HTMLElement>('.ant-checkbox-wrapper');
  if (!wrapper) {
    throw new Error(`checkbox wrapper missing: ${container.innerHTML}`);
  }
  return wrapper;
}

describe('AcpThreadSelectCheckbox', () => {
  it('selects once when the painted checkbox is clicked inside a selectable row', async () => {
    const user = userEvent.setup();
    const { container } = render(<SelectableRow />);

    await user.click(paintedCheckbox(container));

    expect(screen.getByText('on')).toBeInTheDocument();
    expect(screen.getByRole('checkbox')).toBeChecked();
  });

  it('deselects once when the painted checkbox is clicked after the row selected it', async () => {
    const user = userEvent.setup();
    const { container } = render(<SelectableRow />);

    await user.click(screen.getByText('off'));
    expect(screen.getByText('on')).toBeInTheDocument();

    await user.click(paintedCheckbox(container));

    expect(screen.getByText('off')).toBeInTheDocument();
    expect(screen.getByRole('checkbox')).not.toBeChecked();
  });
});
