import * as vscode from "vscode";
import * as config from "./config";
import { DeployController } from "./deployController";
import { DispatchViewProvider } from "./dispatchViewProvider";
import { Finding } from "./findings";
import { FindingsTreeProvider } from "./findingsTreeProvider";
import { ScanController } from "./scanController";

export function activate(context: vscode.ExtensionContext): void {
  const output = vscode.window.createOutputChannel("Altius");
  context.subscriptions.push(output);

  const diagnostics = vscode.languages.createDiagnosticCollection("altius");
  context.subscriptions.push(diagnostics);

  const findingsTree = new FindingsTreeProvider();
  const findingsView = vscode.window.createTreeView("altius.findingsView", {
    treeDataProvider: findingsTree,
  });
  context.subscriptions.push(findingsView);

  const scanController = new ScanController(diagnostics, findingsTree, output);
  const deployController = new DeployController(output);

  const dispatchProvider = new DispatchViewProvider(context.extensionUri, context.secrets, output);
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(DispatchViewProvider.viewType, dispatchProvider),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("altius.runScan", () => scanController.runScan()),
    vscode.commands.registerCommand("altius.clearFindings", () => scanController.clear()),
    vscode.commands.registerCommand("altius.deployDryRun", () => deployController.dryRun()),
    vscode.commands.registerCommand("altius.deployLive", () => deployController.liveDeploy()),
    vscode.commands.registerCommand("altius.focusDispatch", () => dispatchProvider.reveal()),
    vscode.commands.registerCommand("altius.revealFinding", (finding: Finding) =>
      scanController.revealFinding(finding),
    ),
    vscode.commands.registerCommand("altius.setFleetToken", async () => {
      const token = await vscode.window.showInputBox({
        prompt: "Bearer token for the running `altius fleet serve` instance (blank to clear)",
        password: true,
        ignoreFocusOut: true,
      });
      if (token === undefined) return;
      await config.setFleetToken(context.secrets, token);
      void vscode.window.showInformationMessage(
        token ? "Altius: fleet token saved." : "Altius: fleet token cleared.",
      );
    }),
  );
}

export function deactivate(): void {
  // Nothing to tear down: disposables are owned by context.subscriptions.
}
