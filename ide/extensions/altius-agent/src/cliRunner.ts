import { spawn } from "child_process";

export interface CliResult {
  code: number | null;
  stdout: string;
  stderr: string;
}

export interface RunCliOptions {
  cwd?: string;
  onStdoutLine?: (line: string) => void;
  onStderrLine?: (line: string) => void;
}

/** Runs `bin args...` to completion, buffering full stdout/stderr. */
export function runCli(bin: string, args: string[], options: RunCliOptions = {}): Promise<CliResult> {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, { cwd: options.cwd, shell: false });
    let stdout = "";
    let stderr = "";
    let stdoutTail = "";
    let stderrTail = "";

    child.stdout.on("data", (chunk: Buffer) => {
      const text = chunk.toString("utf8");
      stdout += text;
      if (options.onStdoutLine) {
        stdoutTail = emitLines(stdoutTail + text, options.onStdoutLine);
      }
    });
    child.stderr.on("data", (chunk: Buffer) => {
      const text = chunk.toString("utf8");
      stderr += text;
      if (options.onStderrLine) {
        stderrTail = emitLines(stderrTail + text, options.onStderrLine);
      }
    });
    child.on("error", (err) => {
      reject(new Error(`failed to run \`${bin}\`: ${err.message}`));
    });
    child.on("close", (code) => {
      if (stdoutTail && options.onStdoutLine) options.onStdoutLine(stdoutTail);
      if (stderrTail && options.onStderrLine) options.onStderrLine(stderrTail);
      resolve({ code, stdout, stderr });
    });
  });
}

/** Emits complete lines from `buffer` via `onLine`, returning the unterminated remainder. */
function emitLines(buffer: string, onLine: (line: string) => void): string {
  const lines = buffer.split("\n");
  const remainder = lines.pop() ?? "";
  for (const line of lines) onLine(line);
  return remainder;
}
