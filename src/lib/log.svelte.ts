import { commands, events, type LogEntry } from "./bindings";

const MAX = 2000;

export const log = $state({ entries: [] as LogEntry[] });

export async function startLog() {
  log.entries = await commands.logEntries();
  await events.logEvent.listen((e) => {
    log.entries.push(e.payload);
    if (log.entries.length > MAX) log.entries.splice(0, log.entries.length - MAX);
  });
}

export async function clearLog() {
  await commands.clearLog();
  log.entries = [];
}

export function commandLine(e: LogEntry): string {
  return [e.program, ...e.args.map((a) => (/[\s'"|&;<>()$`\\]/.test(a) ? `'${a.replace(/'/g, "'\\''")}'` : a))].join(" ");
}

export function entryText(e: LogEntry): string {
  const lines = [`$ ${commandLine(e)}`, `# exit ${e.exitCode ?? "?"} · ${e.durationMs} ms`];
  if (e.stdout) lines.push(e.stdout.trimEnd());
  if (e.stderr) lines.push(`[stderr]\n${e.stderr.trimEnd()}`);
  return lines.join("\n");
}
