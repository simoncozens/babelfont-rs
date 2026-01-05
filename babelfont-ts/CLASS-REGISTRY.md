# Class Registry - Extending Classes

The babelfont-ts library provides a class registry system that allows users to extend or replace the library's classes with their own custom implementations. This is useful when you want to add custom methods, properties, or decorators to the library's objects.

## Basic Usage

### 1. Create Custom Class Extensions

```typescript
import { Axis, Node } from "babelfont";

// Create your own versions with custom methods
class MyAxis extends Axis {
  getDisplayName(): string {
    return `${this.tag}: ${this.name["dflt"] || ""}`;
  }

  getRange(): number {
    return (this.max || 0) - (this.min || 0);
  }
}

class MyNode extends Node {
  distanceFromOrigin(): number {
    return Math.sqrt(this.x * this.x + this.y * this.y);
  }

  moveTo(x: number, y: number): void {
    this.x = x;
    this.y = y;
  }
}
```

### 2. Create a Class Registry

```typescript
import { ClassRegistry, Font } from "babelfont";

const customRegistry: ClassRegistry = {
  Axis: MyAxis,
  Node: MyNode,
};
```

### 3. Pass Registry to Font Constructor

```typescript
import { Font, ReviverFunc } from "babelfont";
import fs from "fs";

const fileContents = fs.readFileSync("font.babelfont", "utf8");
const rawFont = JSON.parse(fileContents, ReviverFunc);

// Pass the registry when creating the font
const font = new Font(rawFont, customRegistry);

// Now font.axes will contain MyAxis instances
const axis = font.axes![0];
// TypeScript doesn't know this is a MyAxis, so you need to cast:
console.log((axis as MyAxis).getDisplayName()); // Works!

// And paths' nodes will be MyNode instances
const glyph = font.glyphs[0];
const node = glyph.layers![0].shapes![0];
if (!("reference" in node)) {
  const firstNode = node.nodes![0];
  // Cast to access custom methods
  console.log((firstNode as MyNode).distanceFromOrigin()); // Works!
}
```

## TypeScript Type Safety

### The Challenge

When you use a `ClassRegistry`, the objects at runtime are instances of your custom classes (like `MyAxis` or `MyNode`), but TypeScript doesn't know this - it still sees them as the base class types (`Axis`, `Node`).

This is because the registry is resolved at runtime, and TypeScript can't infer the types from it.

### Solution: Type Assertions

Use TypeScript's `as` keyword to assert the correct type:

```typescript
const axis = font.axes![0];
const myAxis = axis as MyAxis;
console.log(myAxis.getDisplayName()); // ✓ TypeScript knows about this method
```

### Advanced: Creating a Typed Font Wrapper

For larger projects, you can create a wrapper class that provides full type inference:

```typescript
class MyTypedFont extends Font {
  // Override with properly typed properties
  declare axes: MyAxis[];
  declare glyphs: MyGlyph[];
  declare names: MyNames;

  constructor(data: any, registry?: ClassRegistry) {
    super(data, registry);
  }

  // Helper methods with proper types
  getAxis(tag: string): MyAxis | undefined {
    return this.axes?.find((a) => a.tag === tag) as MyAxis | undefined;
  }

  getGlyph(name: string): MyGlyph | undefined {
    return this.glyphs.find((g) => g.name === name) as MyGlyph | undefined;
  }
}

// Now you have full type safety:
const font = new MyTypedFont(rawFont, customRegistry);
const axis = font.getAxis("wght");
console.log(axis?.getDisplayName()); // ✓ Full autocomplete!
```

### Alternative: Ambient Module Declaration

You can also use TypeScript module augmentation for global type updates:

```typescript
declare module "babelfont" {
  interface Axis {
    getDisplayName(): string;
    getRange(): number;
  }

  interface Node {
    distanceFromOrigin(): number;
  }
}
```

But this approach requires maintaining type declarations, whereas casting is explicit and doesn't require extra setup.

````

## How It Works

### Registry Resolution

When you provide a `ClassRegistry`:
- Classes specified in the registry are used instead of the defaults
- Classes NOT in the registry fall back to the library defaults
- The registry applies throughout the entire object tree

### Supported Classes

The `ClassRegistry` interface supports all inflatable classes:

```typescript
export interface ClassRegistry {
  Anchor?: typeof Anchor;
  Axis?: typeof Axis;
  Color?: typeof Color;
  Component?: typeof Component;
  Features?: typeof Features;
  Instance?: typeof Instance;
  Names?: typeof Names;
  Position?: typeof Position;
  Guide?: typeof Guide;
  OTValue?: typeof OTValue;
  Master?: typeof Master;
  Node?: typeof Node;
  Path?: typeof Path;
  Layer?: typeof Layer;
  Glyph?: typeof Glyph;
  Font?: typeof Font;
}
````

## Advanced Examples

### Adding TypeScript Support

```typescript
class MyGlyph extends Glyph {
  // Add custom typed properties
  private _metadata: Map<string, any> = new Map();

  setMetadata(key: string, value: any): void {
    this._metadata.set(key, value);
  }

  getMetadata<T>(key: string): T | undefined {
    return this._metadata.get(key);
  }
}

const registry: ClassRegistry = {
  Glyph: MyGlyph,
};

const font = new Font(rawFont, registry);
const glyph = font.glyphs[0] as InstanceType<typeof MyGlyph>;
glyph.setMetadata("favorite", true);
```

### Using Decorators

```typescript
// With TypeScript experimental decorators enabled
function log(target: any, propertyKey: string, descriptor: PropertyDescriptor) {
  const originalMethod = descriptor.value;
  descriptor.value = function (...args: any[]) {
    console.log(`Calling ${propertyKey}`);
    return originalMethod.apply(this, args);
  };
  return descriptor;
}

class MyNode extends Node {
  @log
  distanceFromOrigin(): number {
    return Math.sqrt(this.x * this.x + this.y * this.y);
  }
}
```

### Partial Registries

You don't need to provide all classes - just register the ones you want to customize:

```typescript
const customRegistry: ClassRegistry = {
  Axis: MyAxis, // Custom
  Node: MyNode, // Custom
  // All other classes use defaults
};

const font = new Font(rawFont, customRegistry);
```

## Important Considerations

1. **Constructor Compatibility**: Your custom classes must have constructors compatible with the parent class:

   ```typescript
   // ✓ Correct
   class MyAxis extends Axis {
     constructor(data: IAxis) {
       super(data);
     }
   }

   // ✗ Avoid changing the constructor signature
   class MyAxis extends Axis {
     constructor(data: IAxis, customParam: string) {
       super(data);
     }
   }
   ```

2. **Proxy Compatibility**: Custom classes still get wrapped with the Proxy for camelCase property access, so you don't need to worry about that.

3. **Method Availability**: Methods on your custom classes are available immediately:

   ```typescript
   const axis = font.axes![0];
   axis.getDisplayName(); // ✓ Works (custom method)
   axis.tag; // ✓ Works (inherited property)
   axis.tag_override; // ✓ Works (snake_case access)
   ```

4. **instanceof Checks**: You can use instanceof to check types:
   ```typescript
   if (axis instanceof MyAxis) {
     axis.getDisplayName();
   }
   ```

## Global Registry (Advanced)

For advanced use cases, you can set a global registry that applies to all Font instances:

```typescript
import { setGlobalClassRegistry } from "babelfont";

const globalRegistry: ClassRegistry = {
  Axis: MyAxis,
  Node: MyNode,
};

// Now all fonts use this registry by default
const font1 = new Font(rawFont1); // Uses globalRegistry
const font2 = new Font(rawFont2); // Uses globalRegistry

// You can still override with an instance-specific registry
const font3 = new Font(rawFont3, myCustomRegistry);
```

Note: The global registry feature is intended for advanced scenarios. In most cases, passing the registry to the Font constructor is clearer and more maintainable.
