import { formatChatDateTime } from '@/components/chat/chatTime';
import { AcpAgentIcon } from '@/lib/acpAgentIcon';
import { hasKnownModelIcon, SmartModelIcon } from '@/lib/providerIcons';
import type {
  AcpSessionConfigOption,
  AcpSessionConfigSelectGroup,
  AcpSessionConfigSelectOption,
} from '@/types/acp';

/** True when the option is a boolean toggle (ACP type, boolean currentValue, or empty fast/toggle). */
export function isBooleanConfigOption(option?: AcpSessionConfigOption | null): boolean {
  if (!option) return false;
  if (option.type === 'boolean') return true;
  if (typeof option.currentValue === 'boolean') return true;
  const hasChoices = Array.isArray(option.options) && option.options.length > 0;
  if (hasChoices) return false;
  // Agents sometimes advertise Fast as select with no options; still treat as on/off.
  return /(fast|toggle|enable|enabled|bool)/i.test(`${option.id} ${option.name}`);
}

export function configChoices(option?: AcpSessionConfigOption): AcpSessionConfigSelectOption[] {
  if (!option) return [];
  if (isBooleanConfigOption(option)) {
    return [
      { value: 'true', name: 'On' },
      { value: 'false', name: 'Off' },
    ];
  }
  if (!option.options?.length) return [];
  const first = option.options[0];
  if ('group' in first) {
    return (option.options as AcpSessionConfigSelectGroup[]).flatMap((group) => group.options);
  }
  return option.options as AcpSessionConfigSelectOption[];
}

export function configChoicePayload(
  option: AcpSessionConfigOption,
  value: string,
): string | boolean {
  if (isBooleanConfigOption(option)) return value === 'true';
  return value;
}

export function selectedConfigLabel(option?: AcpSessionConfigOption): string {
  const current = option?.currentValue;
  if (typeof current === 'boolean' || isBooleanConfigOption(option)) {
    const on = current === true || current === 'true';
    return on ? 'On' : 'Off';
  }
  return configChoices(option).find((choice) => String(choice.value) === String(current))?.name
    ?? String(current ?? option?.name ?? '');
}

function modeToken(value: unknown): string {
  const parts = String(value ?? '')
    .trim()
    .toLowerCase()
    .split(/[#/:]/)
    .filter(Boolean);
  return parts[parts.length - 1]?.replace(/_/g, '-') ?? '';
}

export function isPlanModeValue(value: unknown): boolean {
  return modeToken(value) === 'plan';
}

export function isDefaultAgentModeValue(value: unknown): boolean {
  return ['agent', 'default', 'code', 'normal', 'build'].includes(modeToken(value));
}

export function optionContainsPlan(option: AcpSessionConfigOption): boolean {
  return configChoices(option).some((choice) => isPlanModeValue(choice.value));
}

export function isPermissionModeChoice(value: unknown, name?: string): boolean {
  const token = modeToken(value).replace(/[\s_-]/g, '');
  if ([
    'acceptedits',
    'autoedit',
    'auto',
    'dontask',
    'bypasspermissions',
    'yolo',
    'unrestricted',
    'fullaccess',
    'readonly',
  ].includes(token)) return true;
  const label = String(name ?? '').toLowerCase().replace(/[\s_-]/g, '');
  return [
    'acceptedits',
    'autoedit',
    'dontask',
    'bypasspermissions',
    'alwaysapprove',
    'fullaccess',
    'readonly',
    'unrestricted',
  ].some((marker) => label.includes(marker));
}

export function isPermissionOption(option: AcpSessionConfigOption): boolean {
  const identity = `${option.id} ${option.name} ${option.description ?? ''} ${option.category ?? ''}`
    .toLowerCase();
  if (/(permission|approval|allow[_ -]?all|access)/.test(identity)) return true;
  return option.category === 'mode'
    && !optionContainsPlan(option)
    && configChoices(option).some((choice) => isPermissionModeChoice(choice.value, choice.name));
}

export function isRestrictivePermissionChoice(value: unknown, name?: string): boolean {
  const identity = `${String(value ?? '')} ${name ?? ''}`.toLowerCase();
  return /(false|off|default|manual|prompt|request|ask|read[_ -]?only|deny)/.test(identity);
}

export function isFullAccessPermissionChoice(value: unknown, name?: string): boolean {
  const identity = `${String(value ?? '')} ${name ?? ''}`.toLowerCase();
  const compact = identity.replace(/[\s_-]/g, '');
  return compact.includes('bypasspermissions')
    || compact.includes('dangerouslyskippermissions')
    || compact.includes('unrestricted')
    || /(^|\s|[_-])(true|on|full|yolo|allow[_ -]?all)(\s|$|[_-])/.test(identity);
}

export function formatAcpTime(createdAt: string): string {
  const raw = createdAt.trim();
  // Prefer ISO / "YYYY-MM-DD HH:mm:ss" parsing; avoid always-Z so local DB times stay local.
  const ms = Date.parse(
    raw.includes('T')
      ? raw
      : /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}/.test(raw)
        ? raw.replace(' ', 'T')
        : raw,
  );
  if (Number.isFinite(ms)) return formatChatDateTime(ms);
  return raw;
}

/** Prefer model id / value for SmartModelIcon keyword matching. */
export function modelIconKey(choice: { value: string; name: string }): string {
  return String(choice.value || choice.name || '').trim() || 'model';
}

export function isThoughtOption(option: AcpSessionConfigOption): boolean {
  if (option.category === 'thought_level') return true;
  return /(reasoning|thought|effort)/i.test(`${option.id} ${option.name}`);
}

export function isModelOption(option: AcpSessionConfigOption): boolean {
  if (option.category === 'model') return true;
  if (option.category === 'model_config') return false;
  return /(^|[_ -])model($|[_ -])/i.test(`${option.id} ${option.name}`);
}

/** Extra model-side knobs (speed / fast) shown next to model & reasoning. */
export function isModelConfigExtra(option: AcpSessionConfigOption): boolean {
  if (isModelOption(option) || isThoughtOption(option)) return false;
  if (option.category === 'model_config') return true;
  const identity = `${option.id} ${option.name}`.toLowerCase();
  return /(speed|latency|throughput|fast|性能|速度)/.test(identity);
}

/** Rank reasoning levels; higher = stronger. Returns null when unknown. */
function reasoningRank(value: string, name?: string): number | null {
  const hay = `${value} ${name ?? ''}`.toLowerCase().replace(/[_\s-]+/g, '');
  // Order from weakest → strongest (index is rank).
  const order = [
    'none', 'off', 'disable', 'disabled',
    'minimal', 'min',
    'low', 'light',
    'medium', 'med', 'default', 'standard', 'normal',
    'high',
    'xhigh', 'extrahigh', 'veryhigh',
    'max', 'maximum', 'ultra', 'extreme', '最高', '极高',
  ];
  let best: number | null = null;
  for (let i = 0; i < order.length; i += 1) {
    if (hay.includes(order[i])) best = i;
  }
  return best;
}

/** True when the current thought/reasoning choice is the strongest available. */
export function isMaxThoughtLevel(option?: AcpSessionConfigOption | null): boolean {
  if (!option) return false;
  const choices = configChoices(option);
  if (choices.length === 0) return false;
  const ranks = choices.map((choice, index) => {
    const rank = reasoningRank(String(choice.value), choice.name);
    return { value: String(choice.value), rank: rank ?? -1, index };
  });
  const known = ranks.filter((item) => item.rank >= 0);
  if (known.length === 0) {
    // Agents usually list ascending strength — treat last entry as max.
    return String(option.currentValue) === String(choices[choices.length - 1].value);
  }
  const maxRank = Math.max(...known.map((item) => item.rank));
  return known.some(
    (item) => item.rank === maxRank && item.value === String(option.currentValue),
  );
}

export function isSpeedEnabled(option: AcpSessionConfigOption): boolean {
  if (isBooleanConfigOption(option)) {
    return option.currentValue === true || String(option.currentValue) === 'true';
  }
  const current = configChoices(option).find(
    (choice) => String(choice.value) === String(option.currentValue),
  );
  const identity = `${option.currentValue} ${current?.name ?? ''}`.toLowerCase();
  if (/(off|false|standard|normal|default|slow)/.test(identity)) return false;
  return /(fast|priority|turbo|on|true|极速|快速)/.test(identity);
}

export function nextSpeedValue(option: AcpSessionConfigOption): string | boolean {
  const enabled = isSpeedEnabled(option);
  if (isBooleanConfigOption(option)) return !enabled;
  const choices = configChoices(option);
  if (choices.length === 0) return !enabled;
  const isOnChoice = (choice: AcpSessionConfigSelectOption) => {
    const identity = `${choice.value} ${choice.name}`.toLowerCase();
    if (/(off|false|standard|normal|default|slow)/.test(identity)) return false;
    return /(fast|priority|turbo|on|true|极速|快速)/.test(identity);
  };
  if (enabled) {
    return (
      choices.find((choice) => !isOnChoice(choice))
      ?? choices[0]
    ).value;
  }
  return (
    choices.find((choice) => isOnChoice(choice))
    ?? choices[choices.length - 1]
  ).value;
}

/** Model brand icon, falling back to the active ACP agent icon when unknown. */
export function AcpModelChoiceIcon({
  modelId,
  agentId,
  agentName,
  agentIcon,
  size = 16,
}: {
  modelId: string;
  agentId?: string | null;
  agentName?: string;
  agentIcon?: string | null;
  size?: number;
}) {
  if (modelId && hasKnownModelIcon(modelId)) {
    return <SmartModelIcon modelId={modelId} size={size} type="color" />;
  }
  if (agentId) {
    return (
      <AcpAgentIcon
        agentId={agentId}
        agentName={agentName}
        icon={agentIcon}
        size={size}
      />
    );
  }
  return <SmartModelIcon modelId={modelId || 'model'} size={size} type="color" />;
}
