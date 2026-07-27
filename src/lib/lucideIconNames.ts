/**
 * Lucide icon-name conversions. Stored tool icons use kebab-case
 * ("wand-sparkles"), while the `icons` barrel keys are PascalCase
 * ("WandSparkles"). `kebabToPascal(pascalToKebab(x)) === x` holds for every
 * Lucide export, so kebab names are safe as the persisted format.
 */
export function pascalToKebab(name: string): string {
  return name
    // Uppercase runs split letter-by-letter ("AZ" → "a-z") so the round trip
    // stays lossless — kebabToPascal capitalizes exactly one letter per segment.
    .replace(/([A-Z])(?=[A-Z])/g, '$1-')
    .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
    .replace(/([a-zA-Z])([0-9])/g, '$1-$2')
    .toLowerCase();
}

export function kebabToPascal(name: string): string {
  return name
    .split('-')
    .filter(Boolean)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join('');
}
