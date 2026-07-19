import * as path from "path";
import * as vscode from "vscode";
import { runCli } from "./cliRunner";
import * as config from "./config";
import { Finding, ScanReport, Severity } from "./findings";
import { FindingsTreeProvider } from "./findingsTreeProvider";

const SEVERITY_TO_DIAGNOSTIC: Record<Severity, vscode.DiagnosticSeverity> = {
  critical: vscode.DiagnosticSeverity.Error,
  high: vscode.DiagnosticSeverity.Error,
  medium: vscode.DiagnosticSeverity.Warning,
  low: vscode.DiagnosticSeverity.Information,
  info: vscode.DiagnosticSeverity.Hint,
};

export class ScanController {
  constructor(
    private readonly diagnostics: vscode.DiagnosticCollection,
    private readonly tree: FindingsTreeProvider,
    private readonly output: vscode.OutputChannel,
  ) {}

  clear(): void {
    this.diagnostics.clear();
    this.tree.clear();
  }

  async runScan(): Promise<void> {
    const target = config.scanPath();
    const chain = config.scanChain();
    const bin = config.cliPath();

    await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: `Altius: scanning ${target}` },
      async () => {
        this.output.appendLine(`$ ${bin} scan --path ${target} --chain ${chain} --format json`);
        const result = await runCli(bin, [
          "scan",
          "--path",
          target,
          "--chain",
          chain,
          "--format",
          "json",
        ]);

        if (result.stderr.trim()) {
          this.output.appendLine(result.stderr.trim());
        }

        let report: ScanReport;
        try {
          report = JSON.parse(result.stdout) as ScanReport;
        } catch {
          this.output.show(true);
          void vscode.window.showErrorMessage(
            `Altius scan produced no parseable output (exit ${result.code}). See "Altius" output channel.`,
          );
          return;
        }

        this.applyReport(report, target);

        const high = report.findings.filter(
          (f) => f.severity === "high" || f.severity === "critical",
        ).length;
        const message = `Altius scan: ${report.findings.length} finding(s)${
          high ? `, ${high} high/critical` : ""
        }`;
        this.output.appendLine(message);
        if (high > 0) {
          void vscode.window.showWarningMessage(message);
        } else {
          void vscode.window.showInformationMessage(message);
        }
      },
    );
  }

  private applyReport(report: ScanReport, scanRoot: string): void {
    this.diagnostics.clear();
    this.tree.setFindings(report.findings, report.target);

    const byFile = new Map<string, vscode.Diagnostic[]>();
    for (const finding of report.findings) {
      const absolutePath = path.isAbsolute(finding.location.file)
        ? finding.location.file
        : path.resolve(scanRoot, finding.location.file);
      const line = Math.max(0, (finding.location.start_line ?? 1) - 1);
      const endLine = Math.max(line, (finding.location.end_line ?? finding.location.start_line ?? 1) - 1);
      const startCol = Math.max(0, (finding.location.start_column ?? 1) - 1);
      const endCol = Math.max(startCol + 1, (finding.location.end_column ?? finding.location.start_column ?? 2) - 1);
      const range = new vscode.Range(line, startCol, endLine, endCol);

      const diagnostic = new vscode.Diagnostic(
        range,
        `${finding.title}: ${finding.description}`,
        SEVERITY_TO_DIAGNOSTIC[finding.severity],
      );
      diagnostic.source = `altius (${finding.tool})`;
      diagnostic.code = finding.pattern_id;
      const bucket = byFile.get(absolutePath) ?? [];
      bucket.push(diagnostic);
      byFile.set(absolutePath, bucket);
    }

    for (const [file, fileDiagnostics] of byFile) {
      this.diagnostics.set(vscode.Uri.file(file), fileDiagnostics);
    }
  }

  async revealFinding(finding: Finding): Promise<void> {
    const target = this.tree.scannedTarget ?? config.scanPath();
    const absolutePath = path.isAbsolute(finding.location.file)
      ? finding.location.file
      : path.resolve(target, finding.location.file);
    const line = Math.max(0, (finding.location.start_line ?? 1) - 1);
    try {
      const document = await vscode.workspace.openTextDocument(absolutePath);
      const editor = await vscode.window.showTextDocument(document);
      const position = new vscode.Position(line, 0);
      editor.selection = new vscode.Selection(position, position);
      editor.revealRange(new vscode.Range(position, position), vscode.TextEditorRevealType.InCenter);
    } catch (err) {
      void vscode.window.showErrorMessage(`Altius: cannot open ${absolutePath}: ${String(err)}`);
    }
  }
}
