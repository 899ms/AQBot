import { lazy, memo, Suspense, type ComponentType } from 'react';
import {
  BookOpen,
  Brain,
  Code,
  Copy,
  FileQuestion,
  Lightbulb,
  Languages,
  ListCollapse,
  MessageSquare,
  PenLine,
  Search,
  Sparkles,
  SpellCheck,
  Terminal,
  WandSparkles,
  type LucideProps,
} from 'lucide-react';

/**
 * Icons used by the builtin tools and legacy custom-tool presets stay
 * statically bundled so the toolbar renders them instantly; anything else
 * resolves through the async icon catalog.
 */
const STATIC_ICONS: Record<string, ComponentType<LucideProps>> = {
  'wand-sparkles': WandSparkles,
  languages: Languages,
  'spell-check': SpellCheck,
  'list-collapse': ListCollapse,
  brain: Brain,
  'book-open': BookOpen,
  code: Code,
  copy: Copy,
  'file-question': FileQuestion,
  lightbulb: Lightbulb,
  'message-square': MessageSquare,
  'pen-line': PenLine,
  search: Search,
  sparkles: Sparkles,
  terminal: Terminal,
};

const dynamicIcons = new Map<string, ComponentType<LucideProps>>();

function dynamicIcon(name: string): ComponentType<LucideProps> {
  let Icon = dynamicIcons.get(name);
  if (!Icon) {
    Icon = lazy(async () => {
      const { lucideIconByName } = await import('@/lib/lucideIconLibrary');
      return { default: lucideIconByName(name) ?? Sparkles };
    }) as unknown as ComponentType<LucideProps>;
    dynamicIcons.set(name, Icon);
  }
  return Icon;
}

/**
 * Renders a selection-toolbar tool icon by its kebab-case Lucide name, with an
 * inline placeholder (then Sparkles fallback) while unknown icons load.
 */
export const LucideToolIcon = memo(function LucideToolIcon({
  name,
  size = 14,
  className,
}: {
  name: string;
  size?: number;
  className?: string;
}) {
  const StaticIcon = STATIC_ICONS[name];
  if (StaticIcon) return <StaticIcon aria-hidden className={className} size={size} />;
  const DynamicIcon = dynamicIcon(name);
  return (
    <Suspense
      fallback={
        <span
          aria-hidden
          className={className}
          style={{ display: 'inline-block', flex: '0 0 auto', height: size, width: size }}
        />
      }
    >
      <DynamicIcon aria-hidden className={className} size={size} />
    </Suspense>
  );
});
