import {
  Component as IComponent,
  Node as INode,
  Path as IPath,
  Shape,
  NodeType,
} from "./underlying";
import { createCaseConvertingProxy } from "./proxyUtils";
import type { WithCamelCase } from "./typeUtils";
import { getClassConstructor } from "./registry";
import { DecomposedAffine } from "./decomposedAffine";

export interface Component extends WithCamelCase<IComponent> {}
export class Component {
  constructor(data: IComponent) {
    Object.assign(this, data);
    if (data.transform) {
      const DecomposedAffineClass = getClassConstructor(
        "DecomposedAffine",
        DecomposedAffine
      );
      this.transform = new DecomposedAffineClass(data.transform);
    }
    return createCaseConvertingProxy(this, Component.prototype);
  }
}

export interface Node extends WithCamelCase<INode> {
  __parent?: Path;
  nextNode(): Node | undefined;
  previousNode(): Node | undefined;
  nextOnCurveNode(): Node | undefined;
  previousOnCurveNode(): Node | undefined;
}
export class Node {
  constructor(data: INode) {
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Node.prototype);
  }

  get parent(): Path | undefined {
    return this.__parent;
  }
  set parent(value: Path | undefined) {
    this.__parent = value;
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
    const { __parent, ...rest } = this;
    return rest;
  }
}

export interface Path extends WithCamelCase<IPath> {
  nodes: Node[];
}
export class Path {
  constructor(data: IPath) {
    if (data.nodes) {
      const NodeClass = getClassConstructor("Node", Node);
      data.nodes = Path.parseNodes(data.nodes as unknown as string).map((n) => {
        const nObject = new NodeClass(n);
        nObject.parent = this;
        return createCaseConvertingProxy(nObject, NodeClass.prototype) as Node;
      });
    }
    Object.assign(this, data);
    return createCaseConvertingProxy(this, Path.prototype);
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
        })
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
    const { nodes, ...rest } = this;
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
