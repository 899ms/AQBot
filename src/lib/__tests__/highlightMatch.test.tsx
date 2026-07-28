import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import { highlightMatch } from '../highlightMatch';

describe('highlightMatch', () => {
  it('returns plain text when query is empty', () => {
    expect(highlightMatch('Hello world', '')).toBe('Hello world');
    expect(highlightMatch('Hello world', '   ')).toBe('Hello world');
  });

  it('highlights case-insensitive matches', () => {
    const { container } = render(<>{highlightMatch('Hello World', 'world')}</>);
    const marks = container.querySelectorAll('mark');
    expect(marks).toHaveLength(1);
    expect(marks[0].textContent).toBe('World');
    expect(container.textContent).toBe('Hello World');
  });

  it('highlights multiple occurrences', () => {
    const { container } = render(<>{highlightMatch('foo bar foo', 'foo')}</>);
    expect(container.querySelectorAll('mark')).toHaveLength(2);
  });
});
