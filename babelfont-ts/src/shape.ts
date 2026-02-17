import {
  Component as IComponent,
  Node as INode,
  Path as IPath,
  Shape,
  NodeType,
} from "./underlying";
import type { Layer } from "./layer";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import { WithParent, ensureParentAccessors, setParent } from "./parent";
import { getClassConstructor } from "./registry";
import { DecomposedAffine } from "./decomposedAffine";

export interface Component
  extends WithCamelCase<IComponent>, WithParent<Layer> {
  transform: DecomposedAffine;
}
export class Component {
  constructor(data: IComponent) {
    Object.assign(this, data);
    ensureParentAccessors(this);
    if (data.transform) {
      const DecomposedAffineClass = getClassConstructor(
        "DecomposedAffine",
        DecomposedAffine,
      );
      this.transform = new DecomposedAffineClass(data.transform);
    }
    return createCaseConvertingProxy(this, Component.prototype);
  }
}

export interface Node extends WithCamelCase<INode>, WithParent<Path> {
  nextNode(): Node | undefined;
  previousNode(): Node | undefined;
  nextOnCurveNode(): Node | undefined;
  previousOnCurveNode(): Node | undefined;
}
export class Node {
  constructor(data: INode) {
    Object.assign(this, data);
    ensureParentAccessors(this);
    return createCaseConvertingProxy(this, Node.prototype);
  }

  nextNode(): Node | undefined {
    if (!this.parent) return undefined;
    const index = this.parent.nodes.indexOf(this);
    const nextIndex = (index + 1) % this.parent.nodes.length;
    return this.parent.nodes[nextIndex] as Node;
  }

  previousNode(): Node | undefined {
    if (!this.parent) return undefined;
    const index = this.parent.nodes.indexOf(this);
    const prevIndex =
      (index - 1 + this.parent.nodes.length) % this.parent.nodes.length;
    return this.parent.nodes[prevIndex] as Node;
  }

  nextOnCurveNode(): Node | undefined {
    let currentNode: Node | undefined = this;
    do {
      currentNode = currentNode?.nextNode();
      if (currentNode?.nodetype != NodeType.OffCurve) {
        return currentNode;
      }
    } while (currentNode && currentNode !== this);
    return undefined;
  }

  previousOnCurveNode(): Node | undefined {
    let currentNode: Node | undefined = this;
    do {
      currentNode = currentNode?.previousNode();
      if (currentNode?.nodetype != NodeType.OffCurve) {
        return currentNode;
      }
    } while (currentNode && currentNode !== this);
    return undefined;
  }

  toJSON(): INode {
    const { __parent, ...rest } = this as Node;
    return rest;
  }
}

export interface Path extends WithCamelCase<IPath>, WithParent<Layer> {
  nodes: Node[];
}
export class Path {
  constructor(data: IPath) {
    ensureParentAccessors(this);
    if (data.nodes) {
      const NodeClass = getClassConstructor("Node", Node);
      data.nodes = Path.parseNodes(data.nodes as unknown as string).map((n) => {
        const nObject = new NodeClass(n);
        return nObject as Node;
      });
    }
    Object.assign(this, data);
    const proxied = createCaseConvertingProxy(this, Path.prototype) as Path;
    proxied.nodes?.forEach((n) =>
      setParent(n as unknown as WithParent<Path>, proxied),
    );
    return proxied;
  }

  private static parseNodes(nodes: string): Node[] {
    const nodesStr = nodes.trim();
    if (!nodesStr) return [];

    const tokens: string[] = nodesStr.split(/\s+/);
    const nodesArray: Node[] = [];

    for (let i = 0; i + 2 < tokens.length; i += 3) {
      let nodetype = NodeType.Line;
      let smooth = false;
      switch (tokens[i + 2]!) {
        case "m":
          nodetype = NodeType.Move;
          break;
        case "l":
          nodetype = NodeType.Line;
          break;
        case "ls":
          nodetype = NodeType.Line;
          smooth = true;
          break;
        case "o":
          nodetype = NodeType.OffCurve;
          break;
        case "c":
          nodetype = NodeType.Curve;
          break;
        case "cs":
          nodetype = NodeType.Curve;
          smooth = true;
          break;
        case "q":
          nodetype = NodeType.QCurve;
          break;
        case "qs":
          nodetype = NodeType.QCurve;
          smooth = true;
          break;
      }
      nodesArray.push(
        new (getClassConstructor("Node", Node))({
          x: parseFloat(tokens[i]!),
          y: parseFloat(tokens[i + 1]!),
          nodetype,
          smooth,
        }),
      );
    }
    return nodesArray;
  }

  toJSON(): IPath {
    const nodesStr = this.nodes
      .map((n) => {
        let typeChar = "l";
        switch (n.nodetype) {
          case NodeType.Move:
            typeChar = "m";
            break;
          case NodeType.Line:
            typeChar = "l";
            break;
          case NodeType.OffCurve:
            typeChar = "o";
            break;
          case NodeType.Curve:
            typeChar = "c";
            break;
          case NodeType.QCurve:
            typeChar = "q";
            break;
        }
        const smoothChar = n.smooth ? "s" : "";
        return `${n.x} ${n.y} ${typeChar}${smoothChar}`;
      })
      .join(" ");
    const { nodes, __parent, ...rest } = this as Path;
    return {
      ...rest,
      nodes: nodesStr,
    } as unknown as IPath;
  }
}

export function isComponent(shape: Shape): shape is Component {
  return (shape as Component).reference !== undefined;
}
export function isPath(shape: Shape): shape is Path {
  return (shape as Path).nodes !== undefined;
}
