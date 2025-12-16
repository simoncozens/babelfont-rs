/**
 * Type utilities for converting snake_case to camelCase in TypeScript types
 */

/**
 * Convert a string literal type from snake_case to camelCase
 */
type SnakeToCamelCase<S extends string> = S extends `${infer T}_${infer U}`
  ? `${T}${Capitalize<SnakeToCamelCase<U>>}`
  : S;

/**
 * Convert all keys of an object from snake_case to camelCase
 * Creates a new type with both snake_case and camelCase keys
 */
export type CamelCaseKeys<T> = {
  [K in keyof T as K extends string ? SnakeToCamelCase<K> : K]: T[K];
};

/**
 * Combine original type with camelCase keys
 * This allows both snake_case and camelCase access
 * Using a more explicit type that TypeScript can better understand
 */
export type WithCamelCase<T> = {
  [K in keyof T]: T[K];
} & {
  [K in keyof T as K extends string ? SnakeToCamelCase<K> : K]: T[K];
};
