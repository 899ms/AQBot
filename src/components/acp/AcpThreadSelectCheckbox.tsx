import type { SyntheticEvent } from 'react';
import type { CheckboxChangeEvent } from 'antd/es/checkbox';
import { Checkbox } from 'antd';

function stopRowToggle(event: SyntheticEvent) {
  event.stopPropagation();
}

/** Stop label clicks here so the conversation row does not toggle again. */
export function AcpThreadSelectCheckbox({
  checked,
  onCheckedChange,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <span
      className="aqbot-acp-thread-select"
      onClick={stopRowToggle}
      onMouseDown={stopRowToggle}
      onPointerDown={stopRowToggle}
    >
      <Checkbox
        checked={checked}
        onChange={(event: CheckboxChangeEvent) => onCheckedChange(event.target.checked)}
      />
    </span>
  );
}
