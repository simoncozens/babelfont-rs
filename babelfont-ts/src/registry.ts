type AnyConstructor = new (data: any) => any;

export interface ClassRegistry {
  Anchor?: AnyConstructor;
  Axis?: AnyConstructor;
  Color?: AnyConstructor;
  Component?: AnyConstructor;
  DecomposedAffine?: AnyConstructor;
  Features?: AnyConstructor;
  Instance?: AnyConstructor;
  Names?: AnyConstructor;
  Position?: AnyConstructor;
  Guide?: AnyConstructor;
  CustomOTValues?: AnyConstructor;
  Master?: AnyConstructor;
  Node?: AnyConstructor;
  Path?: AnyConstructor;
  Layer?: AnyConstructor;
  Glyph?: AnyConstructor;
  Font?: AnyConstructor;
}

let globalClassRegistry: ClassRegistry = {};

/**
 * Context object that holds the class registry for the current inflation operation.
 * This is used internally to pass the registry through nested object creation.
 */
export class InflationContext {
  static current?: ClassRegistry;

  static with<T>(registry: ClassRegistry, fn: () => T): T {
    const previous = InflationContext.current;
    InflationContext.current = registry;
    try {
      return fn();
    } finally {
      InflationContext.current = previous;
    }
  }
}

/**
 * Get the appropriate class constructor from the current context or use the default
 */
export function getClassConstructor<T extends AnyConstructor>(
  name: keyof ClassRegistry,
  defaultClass: T,
): T {
  const registry = InflationContext.current;
  const customClass = registry?.[name] || globalClassRegistry[name];
  return (customClass as T) || defaultClass;
}
