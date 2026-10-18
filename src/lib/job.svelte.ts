import { events, type CoreError, type ItemStatus, type JobResult, type PauseReason } from "./bindings";

/** The running (or last finished) backup or restore; only one runs at a time. */
export const job = $state({
  kind: null as "backup" | "restore" | null,
  running: false,
  /** Packing (backup) or unpacking (restore) the single-file backup. */
  packing: false,
  items: {} as Record<string, ItemStatus>,
  bytesDone: 0,
  totalBytes: 0,
  current: null as { package: string; step: string } | null,
  speed: 0,
  /** Waiting for the phone to come back or be unlocked. */
  paused: null as PauseReason | null,
  result: null as JobResult | null,
  error: null as CoreError | null,
});

const finishedListeners: (() => void)[] = [];

/** Called when a job finishes or fails (e.g. to switch to the Result tab). */
export function onJobFinished(f: () => void) {
  finishedListeners.push(f);
}

let listening = false;

export async function startJobEvents() {
  if (listening) return;
  listening = true;
  await events.jobProgressEvent.listen((e) => {
    const ev = e.payload;
    switch (ev.kind) {
      case "started":
        job.totalBytes = ev.totalBytes;
        job.packing = false;
        job.items = Object.fromEntries(ev.packages.map((p) => [p, "pending" as ItemStatus]));
        break;
      case "item":
        job.items[ev.package] = ev.status;
        break;
      case "progress":
        job.bytesDone = ev.bytesDone;
        job.totalBytes = ev.totalBytes;
        job.current = { package: ev.package, step: ev.step };
        job.speed = ev.speed;
        break;
      case "packing":
        job.packing = true;
        break;
      case "paused":
        job.paused = ev.reason;
        break;
      case "resumed":
        job.paused = null;
        break;
      case "finished":
        job.paused = null;
        job.result = ev.result;
        job.running = false;
        job.packing = false;
        job.bytesDone = job.totalBytes;
        finishedListeners.forEach((f) => f());
        break;
      case "failed":
        job.paused = null;
        job.error = ev.error;
        job.running = false;
        job.packing = false;
        finishedListeners.forEach((f) => f());
        break;
    }
  });
}

/** Clears the previous job before starting a new one of `kind`. */
export function beginJob(kind: "backup" | "restore") {
  job.kind = kind;
  job.result = null;
  job.error = null;
  job.items = {};
  job.bytesDone = 0;
  job.totalBytes = 0;
  job.current = null;
  job.packing = false;
  job.paused = null;
  job.running = true;
}

export function clearJob() {
  job.kind = null;
  job.result = null;
  job.error = null;
  job.items = {};
}
