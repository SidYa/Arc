// Running a backend job from any window: progress and completion come as events for all windows,
// each window follows only the ids it started.
import { api, type JobFinished, type JobProgress, type JobSpec } from "./api";
import { t } from "./i18n.svelte";

export type JobState = "running" | "done" | "error" | "cancelled";

export interface Job {
  id: number;
  spec: JobSpec;
  state: JobState;
  done: number;
  total: number;
  current: string;
  /** Bytes per second, smoothed */
  speed: number;
  error: string | null;
  output: string | null;
  /** Last progress sample, for the speed */
  sample: { time: number; done: number } | null;
}

/** Windows number their jobs independently: random ids don't collide */
const newId = () => crypto.getRandomValues(new Uint32Array(1))[0];

const running = new Map<number, { job: Job; resolve: (job: Job) => void }>();

/** Starts the job; `job` is reactive when the caller passes a $state proxy through `track` */
export function startJob(spec: JobSpec, track: (job: Job) => Job = (j) => j): { job: Job; finished: Promise<Job> } {
  const job = track({ id: newId(), spec, state: "running", done: 0, total: 0, current: "", speed: 0, error: null, output: null, sample: null });
  const finished = new Promise<Job>((resolve) => running.set(job.id, { job, resolve }));
  api.startJob(job.id, spec).catch((e) => onJobFinished({ id: job.id, ok: false, error: String(e), output: null }));
  return { job, finished };
}

export function onJobProgress(list: JobProgress[]) {
  const now = performance.now();
  for (const p of list) {
    const job = running.get(p.id)?.job;
    if (!job) continue;
    if (job.sample) {
      const seconds = (now - job.sample.time) / 1000;
      if (seconds > 0.05) {
        const current = Math.max(0, (p.done - job.sample.done) / seconds);
        job.speed = job.speed ? job.speed * 0.75 + current * 0.25 : current;
      }
    }
    job.sample = { time: now, done: p.done };
    job.done = p.done;
    job.total = p.total;
    job.current = p.current;
  }
}

export function onJobFinished(f: JobFinished) {
  const entry = running.get(f.id);
  if (!entry) return;
  running.delete(f.id);
  const job = entry.job;
  job.state = f.ok ? "done" : f.error === "cancelled" ? "cancelled" : "error";
  job.error = f.error;
  job.output = f.output;
  if (job.state === "done") job.done = Math.max(job.done, job.total);
  entry.resolve(job);
}

export const isPasswordError = (e: string | null) => e === "password_required" || e === "wrong_password";

/** Error codes from Rust become translated text; other messages stay as they are */
export function errorText(e: unknown): string {
  const code = String(e);
  const text = t(`err_${code}`);
  return text === `err_${code}` ? code : text;
}
