import * as vscode from "vscode";
import { compareBySeverity, Finding, Severity } from "./findings";

type TreeNode = SeverityGroupNode | FindingNode;

class SeverityGroupNode {
  readonly kind = "group" as const;
  constructor(
    readonly severity: Severity,
    readonly findings: Finding[],
  ) {}
}

class FindingNode {
  readonly kind = "finding" as const;
  constructor(readonly finding: Finding) {}
}

const SEVERITY_ICONS: Record<Severity, vscode.ThemeIcon> = {
  critical: new vscode.ThemeIcon("error", new vscode.ThemeColor("errorForeground")),
  high: new vscode.ThemeIcon("error"),
  medium: new vscode.ThemeIcon("warning"),
  low: new vscode.ThemeIcon("info"),
  info: new vscode.ThemeIcon("info"),
};

export class FindingsTreeProvider implements vscode.TreeDataProvider<TreeNode> {
  private readonly onDidChangeTreeDataEmitter = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.onDidChangeTreeDataEmitter.event;

  private findings: Finding[] = [];
  private lastScannedTarget: string | undefined;

  setFindings(findings: Finding[], target?: string): void {
    this.findings = [...findings].sort(compareBySeverity);
    this.lastScannedTarget = target;
    this.onDidChangeTreeDataEmitter.fire();
  }

  clear(): void {
    this.findings = [];
    this.lastScannedTarget = undefined;
    this.onDidChangeTreeDataEmitter.fire();
  }

  getTreeItem(node: TreeNode): vscode.TreeItem {
    if (node.kind === "group") {
      const item = new vscode.TreeItem(
        `${capitalize(node.severity)} (${node.findings.length})`,
        vscode.TreeItemCollapsibleState.Expanded,
      );
      item.iconPath = SEVERITY_ICONS[node.severity];
      item.contextValue = "altiusSeverityGroup";
      return item;
    }
    const finding = node.finding;
    const item = new vscode.TreeItem(finding.title, vscode.TreeItemCollapsibleState.None);
    const location = finding.location.start_line
      ? `${finding.location.file}:${finding.location.start_line}`
      : finding.location.file;
    item.description = location;
    item.tooltip = new vscode.MarkdownString(
      `**${finding.title}** (${finding.severity}, ${finding.confidence} confidence)\n\n` +
        `${finding.description}\n\n` +
        (finding.recommendation ? `_Recommendation:_ ${finding.recommendation}\n\n` : "") +
        `\`${finding.pattern_id}\` · ${location}`,
    );
    item.iconPath = SEVERITY_ICONS[finding.severity];
    item.contextValue = "altiusFinding";
    item.command = {
      command: "altius.revealFinding",
      title: "Reveal Finding",
      arguments: [finding],
    };
    return item;
  }

  getChildren(node?: TreeNode): TreeNode[] {
    if (!node) {
      const groups = new Map<Severity, Finding[]>();
      for (const finding of this.findings) {
        const bucket = groups.get(finding.severity) ?? [];
        bucket.push(finding);
        groups.set(finding.severity, bucket);
      }
      return [...groups.entries()]
        .sort(([a], [b]) => compareBySeverity({ severity: a } as Finding, { severity: b } as Finding))
        .map(([severity, findings]) => new SeverityGroupNode(severity, findings));
    }
    if (node.kind === "group") {
      return node.findings.map((finding) => new FindingNode(finding));
    }
    return [];
  }

  get scannedTarget(): string | undefined {
    return this.lastScannedTarget;
  }

  get count(): number {
    return this.findings.length;
  }
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}
