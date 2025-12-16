/**
 * Utility functions for creating proxied objects that convert snake_case to camelCase
 */

/**
 * Convert a camelCase string to snake_case
 */
function camelToSnake(str: string): string {
  return str.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
}

/**
 * Convert a snake_case string to camelCase
 */
function snakeToCamel(str: string): string {
  return str.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
}

/**
 * Create a proxy object that automatically converts camelCase property access
 * to snake_case for the underlying data, while preserving methods defined on
 * the class prototype.
 */
export function createCaseConvertingProxy<T extends object>(
  target: T,
  prototype: any
): T {
  return new Proxy(target, {
    get(obj: any, prop: string | symbol) {
      // Handle symbols (like Symbol.iterator)
      if (typeof prop === "symbol") {
        return obj[prop];
      }

      // First check if it's a method on the prototype
      if (prototype && prop in prototype) {
        const value = prototype[prop];
        // If it's a function, bind it to the target object
        if (typeof value === "function") {
          return value.bind(obj);
        }
      }

      // Check if the property exists as-is (camelCase or already snake_case)
      if (prop in obj) {
        return obj[prop];
      }

      // Try converting camelCase to snake_case
      const snakeCaseProp = camelToSnake(prop);
      if (snakeCaseProp !== prop && snakeCaseProp in obj) {
        return obj[snakeCaseProp];
      }

      // Return undefined for non-existent properties
      return undefined;
    },

    set(obj: any, prop: string | symbol, value: any) {
      if (typeof prop === "symbol") {
        obj[prop] = value;
        return true;
      }

      // Check if property exists as-is
      if (prop in obj) {
        obj[prop] = value;
        return true;
      }

      // Try converting camelCase to snake_case
      const snakeCaseProp = camelToSnake(prop);
      if (snakeCaseProp !== prop && snakeCaseProp in obj) {
        obj[snakeCaseProp] = value;
        return true;
      }

      // For new properties, just set them as-is
      obj[prop] = value;
      return true;
    },

    has(obj: any, prop: string | symbol) {
      if (typeof prop === "symbol") {
        return prop in obj;
      }

      // Check if property exists as-is
      if (prop in obj) {
        return true;
      }

      // Check if it exists in prototype
      if (prototype && prop in prototype) {
        return true;
      }

      // Try converting camelCase to snake_case
      const snakeCaseProp = camelToSnake(prop);
      return snakeCaseProp !== prop && snakeCaseProp in obj;
    },

    ownKeys(obj: any) {
      // Return all keys converted to camelCase
      const keys = Reflect.ownKeys(obj);
      const camelKeys = keys.map((key) => {
        if (typeof key === "string") {
          return snakeToCamel(key);
        }
        return key;
      });
      return camelKeys;
    },

    getOwnPropertyDescriptor(obj: any, prop: string | symbol) {
      if (typeof prop === "symbol") {
        return Reflect.getOwnPropertyDescriptor(obj, prop);
      }

      // Check if property exists as-is
      let descriptor = Reflect.getOwnPropertyDescriptor(obj, prop);
      if (descriptor) {
        return descriptor;
      }

      // Try converting camelCase to snake_case
      const snakeCaseProp = camelToSnake(prop);
      if (snakeCaseProp !== prop) {
        descriptor = Reflect.getOwnPropertyDescriptor(obj, snakeCaseProp);
        if (descriptor) {
          // Return a descriptor with the camelCase property name
          return {
            ...descriptor,
            configurable: true,
            enumerable: true,
          };
        }
      }

      return undefined;
    },
  });
}
