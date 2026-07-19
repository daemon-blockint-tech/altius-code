import * as vscode from "vscode";
import { runCli } from "./cliRunner";
import * as config from "./config";

const CONFIRM_PHRASE = "DEPLOY";

export class DeployController {
  constructor(private readonly output: vscode.OutputChannel) {}

  /** `altius deploy --dry-run`: policy + mandatory simulation only, `FailClosed` guarantees nothing is ever signed. */
  async dryRun(): Promise<void> {
    await this.runDeploy(["--dry-run"], "dry run (policy + simulate only, nothing signed)");
  }

  /**
   * `altius deploy --yes`: runs the full guarded pipeline
   * (policy → simulate → diff → approve → audit → sign) with `AutoApprove`
   * in place of the CLI's interactive terminal prompt, since a spawned,
   * non-TTY child process cannot answer that prompt. This is the same
   * `--yes` flag documented for headless/CI use in
   * crates/altius-cli/src/deploy_command.rs — TxGuard's policy, simulation,
   * and audit trail still run unchanged; only the interactive y/N step
   * moves to this confirmation dialog instead.
   */
  async liveDeploy(): Promise<void> {
    const warned = await vscode.window.showWarningMessage(
      "This signs and broadcasts real transactions through TxGuard (policy + simulation still apply, but nothing is dry-run). This cannot be undone.",
      { modal: true },
      "Continue",
    );
    if (warned !== "Continue") return;

    const typed = await vscode.window.showInputBox({
      prompt: `Type ${CONFIRM_PHRASE} to confirm signing and broadcasting`,
      placeHolder: CONFIRM_PHRASE,
      ignoreFocusOut: true,
    });
    if (typed !== CONFIRM_PHRASE) {
      void vscode.window.showInformationMessage("Altius: deploy cancelled (confirmation text did not match).");
      return;
    }

    await this.runDeploy(["--yes"], "live (signing and broadcasting via TxGuard)");
  }

  private async runDeploy(extraArgs: string[], label: string): Promise<void> {
    const bin = config.cliPath();
    const project = config.deployProjectPath();
    const args = ["deploy", "--project", project, ...extraArgs];

    this.output.show(true);
    this.output.appendLine(`\n$ ${bin} ${args.join(" ")}`);
    this.output.appendLine(`(${label})`);

    await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: `Altius: deploy (${label})` },
      async () => {
        const result = await runCli(bin, args, {
          cwd: project,
          onStdoutLine: (line) => this.output.appendLine(line),
          onStderrLine: (line) => this.output.appendLine(line),
        });

        if (result.code === 0) {
          void vscode.window.showInformationMessage(`Altius deploy (${label}) finished.`);
        } else {
          void vscode.window.showErrorMessage(
            `Altius deploy (${label}) failed (exit ${result.code}). See "Altius" output channel.`,
          );
        }
      },
    );
  }
}
