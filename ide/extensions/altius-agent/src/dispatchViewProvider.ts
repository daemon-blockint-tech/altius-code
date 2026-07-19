import * as vscode from "vscode";
import { BeeAcpClient, Run, userTextMessage } from "./beeacpClient";
import * as config from "./config";

type FromWebviewMessage =
  | { type: "dispatch"; agentName: string; prompt: string }
  | { type: "refresh" }
  | { type: "selectRun"; runId: string }
  | { type: "resume"; runId: string; approved: boolean; note?: string }
  | { type: "cancel"; runId: string };

type ToWebviewMessage =
  | { type: "runs"; runs: Run[] }
  | { type: "selected"; run: Run | null }
  | { type: "error"; message: string }
  | { type: "dispatching"; value: boolean };

const POLL_INTERVAL_MS = 2000;

export class DispatchViewProvider implements vscode.WebviewViewProvider {
  static readonly viewType = "altius.dispatchView";

  private view: vscode.WebviewView | undefined;
  private pollHandle: ReturnType<typeof setInterval> | undefined;
  private selectedRunId: string | undefined;

  constructor(
    private readonly extensionUri: vscode.Uri,
    private readonly secrets: vscode.SecretStorage,
    private readonly output: vscode.OutputChannel,
  ) {}

  resolveWebviewView(webviewView: vscode.WebviewView): void {
    this.view = webviewView;
    webviewView.webview.options = { enableScripts: true, localResourceRoots: [this.extensionUri] };
    webviewView.webview.html = this.getHtml(webviewView.webview);

    webviewView.webview.onDidReceiveMessage((message: FromWebviewMessage) => {
      void this.handleMessage(message);
    });

    webviewView.onDidChangeVisibility(() => {
      if (webviewView.visible) {
        this.startPolling();
      } else {
        this.stopPolling();
      }
    });
    webviewView.onDidDispose(() => this.stopPolling());

    this.startPolling();
  }

  reveal(): void {
    this.view?.show?.(true);
  }

  private async client(): Promise<BeeAcpClient> {
    return new BeeAcpClient({ baseUrl: config.fleetUrl(), token: await config.fleetToken(this.secrets) });
  }

  private startPolling(): void {
    this.stopPolling();
    void this.refresh();
    this.pollHandle = setInterval(() => void this.refresh(), POLL_INTERVAL_MS);
  }

  private stopPolling(): void {
    if (this.pollHandle) clearInterval(this.pollHandle);
    this.pollHandle = undefined;
  }

  private post(message: ToWebviewMessage): void {
    void this.view?.webview.postMessage(message);
  }

  private async refresh(): Promise<void> {
    if (!this.view?.visible) return;
    try {
      const client = await this.client();
      const runs = await client.listRuns();
      runs.sort((a, b) => b.created_at.localeCompare(a.created_at));
      this.post({ type: "runs", runs });
      if (this.selectedRunId) {
        const selected = runs.find((run) => run.run_id === this.selectedRunId) ?? null;
        this.post({ type: "selected", run: selected });
      }
    } catch (err) {
      this.post({ type: "error", message: `Cannot reach fleet at ${config.fleetUrl()}: ${String(err)}` });
    }
  }

  private async handleMessage(message: FromWebviewMessage): Promise<void> {
    try {
      const client = await this.client();
      switch (message.type) {
        case "dispatch": {
          if (!message.prompt.trim()) return;
          this.post({ type: "dispatching", value: true });
          const run = await client.createRun(message.agentName, [userTextMessage(message.prompt)]);
          this.selectedRunId = run.run_id;
          this.post({ type: "dispatching", value: false });
          await this.refresh();
          break;
        }
        case "refresh":
          await this.refresh();
          break;
        case "selectRun":
          this.selectedRunId = message.runId;
          await this.refresh();
          break;
        case "resume":
          await client.resumeRun(message.runId, {
            decision: { approved: message.approved, note: message.note },
          });
          await this.refresh();
          break;
        case "cancel":
          await client.cancelRun(message.runId);
          await this.refresh();
          break;
      }
    } catch (err) {
      this.output.appendLine(`dispatch error: ${String(err)}`);
      this.post({ type: "error", message: String(err) });
      this.post({ type: "dispatching", value: false });
    }
  }

  private getHtml(webview: vscode.Webview): string {
    const nonce = getNonce();
    const csp = [
      `default-src 'none'`,
      `img-src ${webview.cspSource}`,
      `style-src ${webview.cspSource} 'unsafe-inline'`,
      `script-src 'nonce-${nonce}'`,
    ].join("; ");

    return /* html */ `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta http-equiv="Content-Security-Policy" content="${csp}" />
  <style>
    body { font-family: var(--vscode-font-family); font-size: var(--vscode-font-size); color: var(--vscode-foreground); padding: 0 8px; }
    textarea, select, input { width: 100%; box-sizing: border-box; background: var(--vscode-input-background); color: var(--vscode-input-foreground); border: 1px solid var(--vscode-input-border, transparent); border-radius: 4px; padding: 4px 6px; font-family: inherit; font-size: inherit; }
    textarea { resize: vertical; min-height: 3.5em; margin-top: 6px; }
    .row { display: flex; gap: 6px; margin-top: 6px; align-items: center; }
    button { background: var(--vscode-button-background); color: var(--vscode-button-foreground); border: none; border-radius: 4px; padding: 4px 10px; cursor: pointer; }
    button:hover { background: var(--vscode-button-hoverBackground); }
    button.secondary { background: var(--vscode-button-secondaryBackground); color: var(--vscode-button-secondaryForeground); }
    h4 { margin: 14px 0 4px; opacity: 0.75; text-transform: uppercase; font-size: 0.75em; letter-spacing: 0.04em; }
    .run { padding: 6px 4px; border-bottom: 1px solid var(--vscode-panel-border); cursor: pointer; }
    .run:hover { background: var(--vscode-list-hoverBackground); }
    .run.selected { background: var(--vscode-list-activeSelectionBackground); color: var(--vscode-list-activeSelectionForeground); }
    .badge { font-size: 0.75em; padding: 1px 6px; border-radius: 8px; background: var(--vscode-badge-background); color: var(--vscode-badge-foreground); }
    .error { color: var(--vscode-errorForeground); font-size: 0.9em; white-space: pre-wrap; }
    .detail { white-space: pre-wrap; font-size: 0.9em; margin-top: 6px; }
    .approval { border: 1px solid var(--vscode-inputValidation-warningBorder, var(--vscode-panel-border)); background: var(--vscode-inputValidation-warningBackground, transparent); border-radius: 4px; padding: 8px; margin-top: 8px; }
  </style>
</head>
<body>
  <textarea id="prompt" placeholder="@Browser open https://example.com and summarize"></textarea>
  <div class="row">
    <select id="agent">
      <option value="altius">altius</option>
      <option value="security">security</option>
      <option value="browser">browser</option>
    </select>
    <button id="send">Dispatch</button>
    <button id="refresh" class="secondary">Refresh</button>
  </div>
  <div id="error" class="error"></div>
  <h4>Runs</h4>
  <div id="runs"></div>
  <div id="detail"></div>

  <script nonce="${nonce}">
    const vscode = acquireVsCodeApi();
    const runsEl = document.getElementById("runs");
    const detailEl = document.getElementById("detail");
    const errorEl = document.getElementById("error");
    const promptEl = document.getElementById("prompt");
    const agentEl = document.getElementById("agent");
    let runs = [];
    let selectedRun = null;

    document.getElementById("send").addEventListener("click", () => {
      const prompt = promptEl.value.trim();
      if (!prompt) return;
      vscode.postMessage({ type: "dispatch", agentName: agentEl.value, prompt });
      promptEl.value = "";
    });
    document.getElementById("refresh").addEventListener("click", () => {
      vscode.postMessage({ type: "refresh" });
    });

    function statusBadge(status) {
      return '<span class="badge">' + status + '</span>';
    }

    function renderRuns() {
      runsEl.innerHTML = "";
      if (!runs.length) {
        runsEl.innerHTML = '<div style="opacity:0.6">No runs yet.</div>';
        return;
      }
      for (const run of runs) {
        const div = document.createElement("div");
        div.className = "run" + (selectedRun && selectedRun.run_id === run.run_id ? " selected" : "");
        div.innerHTML = '<strong>' + run.agent_name + '</strong> ' + statusBadge(run.status) +
          '<div style="font-size:0.8em;opacity:0.7">' + run.run_id.slice(0, 8) + ' · ' + new Date(run.created_at).toLocaleTimeString() + '</div>';
        div.addEventListener("click", () => {
          selectedRun = run;
          vscode.postMessage({ type: "selectRun", runId: run.run_id });
          renderRuns();
          renderDetail();
        });
        runsEl.appendChild(div);
      }
    }

    function textOf(messages) {
      return (messages || [])
        .flatMap((m) => (m.parts || []).map((p) => p.content))
        .join("\\n");
    }

    function renderDetail() {
      if (!selectedRun) {
        detailEl.innerHTML = "";
        return;
      }
      let html = '<h4>' + selectedRun.status + '</h4><div class="detail">' + escapeHtml(textOf(selectedRun.output) || textOf(selectedRun.input)) + '</div>';
      if (selectedRun.error) {
        html += '<div class="error">' + escapeHtml(selectedRun.error) + '</div>';
      }
      if (selectedRun.status === "awaiting" && selectedRun.approval) {
        const approval = selectedRun.approval;
        html += '<div class="approval"><strong>' + escapeHtml(approval.summary || "Approval required") + '</strong>';
        if (approval.reason) html += '<div>' + escapeHtml(approval.reason) + '</div>';
        html += '<div class="row"><button id="approve">Resume</button><button id="deny" class="secondary">Deny</button></div></div>';
      }
      if (selectedRun.status === "in-progress" || selectedRun.status === "awaiting") {
        html += '<div class="row"><button id="cancel" class="secondary">Cancel</button></div>';
      }
      detailEl.innerHTML = html;

      const approve = document.getElementById("approve");
      if (approve) approve.addEventListener("click", () => {
        vscode.postMessage({ type: "resume", runId: selectedRun.run_id, approved: true });
      });
      const deny = document.getElementById("deny");
      if (deny) deny.addEventListener("click", () => {
        vscode.postMessage({ type: "resume", runId: selectedRun.run_id, approved: false });
      });
      const cancel = document.getElementById("cancel");
      if (cancel) cancel.addEventListener("click", () => {
        vscode.postMessage({ type: "cancel", runId: selectedRun.run_id });
      });
    }

    function escapeHtml(text) {
      const div = document.createElement("div");
      div.textContent = text || "";
      return div.innerHTML;
    }

    window.addEventListener("message", (event) => {
      const message = event.data;
      switch (message.type) {
        case "runs":
          runs = message.runs;
          if (selectedRun) {
            selectedRun = runs.find((r) => r.run_id === selectedRun.run_id) || selectedRun;
          }
          renderRuns();
          renderDetail();
          errorEl.textContent = "";
          break;
        case "selected":
          selectedRun = message.run;
          renderDetail();
          break;
        case "error":
          errorEl.textContent = message.message;
          break;
        case "dispatching":
          document.getElementById("send").disabled = message.value;
          break;
      }
    });
  </script>
</body>
</html>`;
  }
}

function getNonce(): string {
  let text = "";
  const possible = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  for (let i = 0; i < 32; i++) {
    text += possible.charAt(Math.floor(Math.random() * possible.length));
  }
  return text;
}
