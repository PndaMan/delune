import type { ShareFolder } from "@/lib/api"

export type TreeNode = {
  /** Full virtual path of this node. */
  path: string
  name: string
  depth: number
  /** Set when this path is itself a shared folder with files. */
  folder?: ShareFolder
  children: TreeNode[]
  /** Shared folders at or below this node. */
  count: number
}

/** Hidden share roots like `@@abcde` mean nothing to people. */
export const displayName = (segment: string) => (segment.startsWith("@@") ? segment.slice(2) || "Shares" : segment)

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" })

/** Turn a flat folder list into a tree by path segment. */
export function buildTree(folders: ShareFolder[]): TreeNode[] {
  const root: TreeNode = { path: "", name: "", depth: -1, children: [], count: 0 }
  const index = new Map<string, TreeNode>([["", root]])

  for (const folder of folders) {
    const segments = folder.path.split("\\").filter(Boolean)
    let parent = root
    let path = ""
    segments.forEach((segment, i) => {
      path = path ? `${path}\\${segment}` : segment
      let node = index.get(path)
      if (!node) {
        node = { path, name: segment, depth: i, children: [], count: 0 }
        index.set(path, node)
        parent.children.push(node)
      }
      parent = node
    })
    parent.folder = folder
  }

  const finish = (node: TreeNode): number => {
    node.children.sort((a, b) => collator.compare(a.name, b.name))
    node.count = (node.folder ? 1 : 0) + node.children.reduce((sum, child) => sum + finish(child), 0)
    return node.count
  }
  finish(root)
  return root.children
}

/** Paths to open at first: follow single-child chains so the first screen shows something to pick. */
export function initiallyOpen(nodes: TreeNode[]): Set<string> {
  const open = new Set<string>()
  let level = nodes
  while (level.length === 1 && level[0].children.length > 0) {
    open.add(level[0].path)
    level = level[0].children
  }
  return open
}

/**
 * The rows to draw: nodes whose ancestors are all open. With a filter, only branches
 * leading to a matching folder, all opened.
 */
export function visibleRows(nodes: TreeNode[], open: Set<string>, filter: string): TreeNode[] {
  const needle = filter.trim().toLowerCase()
  // With a filter: every path that matches, plus its ancestors, computed once.
  let keep: Set<string> | null = null
  if (needle) {
    keep = new Set()
    const mark = (level: TreeNode[]) => {
      for (const node of level) {
        if (node.path.toLowerCase().includes(needle)) {
          const segments = node.path.split("\\")
          for (let i = 1; i <= segments.length; i++) keep!.add(segments.slice(0, i).join("\\"))
        }
        mark(node.children)
      }
    }
    mark(nodes)
  }

  const rows: TreeNode[] = []
  const walk = (level: TreeNode[]) => {
    for (const node of level) {
      if (keep && !keep.has(node.path)) continue
      rows.push(node)
      if (keep || open.has(node.path)) walk(node.children)
    }
  }
  walk(nodes)
  return rows
}
