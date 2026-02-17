export interface WithParent<TParent> {
  __parent?: TParent;
  parent?: TParent;
}

/**
 * Ensure the parent accessors exist and are non-enumerable so they do not appear in JSON output.
 */
export function ensureParentAccessors<TParent>(obj: WithParent<TParent>): void {
  if (!Object.prototype.hasOwnProperty.call(obj, "__parent")) {
    Object.defineProperty(obj, "__parent", {
      value: undefined,
      writable: true,
      enumerable: false,
      configurable: true,
    });
  }
  if (!Object.prototype.hasOwnProperty.call(obj, "parent")) {
    Object.defineProperty(obj, "parent", {
      get() {
        return (this as WithParent<TParent>).__parent;
      },
      set(value: TParent | undefined) {
        (this as WithParent<TParent>).__parent = value;
      },
      enumerable: false,
      configurable: true,
    });
  }
}

/**
 * Set the parent on an object, defining hidden accessors if needed.
 */
export function setParent<TParent>(
  obj: WithParent<TParent>,
  parent: TParent | undefined,
): void {
  ensureParentAccessors(obj);
  obj.parent = parent;
}
