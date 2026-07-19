import * as vscode from "vscode";

const SECTION = "altius";
const TOKEN_SECRET_KEY = "altius.fleetToken";

export function workspaceRoot(): string | undefined {
  return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
}

export function cliPath(): string {
  return vscode.workspace.getConfiguration(SECTION).get<string>("cliPath", "altius");
}

export function scanPath(): string {
  const root = workspaceRoot();
  const relative = vscode.workspace.getConfiguration(SECTION).get<string>("scanPath", "");
  if (!root) return relative || ".";
  return relative ? joinPath(root, relative) : root;
}

export function scanChain(): string {
  return vscode.workspace.getConfiguration(SECTION).get<string>("scanChain", "auto");
}

export function deployProjectPath(): string {
  const root = workspaceRoot();
  const relative = vscode.workspace
    .getConfiguration(SECTION)
    .get<string>("deployProjectPath", "");
  if (!root) return relative || ".";
  return relative ? joinPath(root, relative) : root;
}

export function fleetUrl(): string {
  return vscode.workspace
    .getConfiguration(SECTION)
    .get<string>("fleetUrl", "http://127.0.0.1:8788")
    .replace(/\/+$/, "");
}

export async function fleetToken(secrets: vscode.SecretStorage): Promise<string | undefined> {
  return secrets.get(TOKEN_SECRET_KEY);
}

export async function setFleetToken(
  secrets: vscode.SecretStorage,
  token: string,
): Promise<void> {
  if (token) {
    await secrets.store(TOKEN_SECRET_KEY, token);
  } else {
    await secrets.delete(TOKEN_SECRET_KEY);
  }
}

function joinPath(root: string, relative: string): string {
  const sep = root.endsWith("/") || root.endsWith("\\") ? "" : "/";
  return `${root}${sep}${relative}`;
}
