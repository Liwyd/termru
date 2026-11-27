// Scrolls the windowed hosts table of the mock harness and reports frame times and
// main-thread time, through the Chrome DevTools protocol.
//
//   npm run dev                       # in apps/desktop, another terminal
//   node scripts/scroll-perf.mjs 'http://127.0.0.1:1420/?mock&many=10000#hosts' [seconds] [px-per-frame]
//
// Seconds default to 4, px per frame to 240 (14,000 px/s at 60 Hz: far past a hard
// wheel flick). PROFILE=1 also prints the top JavaScript self times. Needs
// google-chrome and Node 22 (the global WebSocket). Builds nothing, starts only
// a headless Chrome and stops it. Numbers: docs/performance.md.
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const url = process.argv[2];
const seconds = Number(process.argv[3] ?? 4);
const step = Number(process.argv[4] ?? 240);
const port = 9333;
const profile = mkdtempSync(join(tmpdir(), "perf-"));
const chrome = spawn(
  "google-chrome",
  [
    "--headless=new",
    "--disable-gpu",
    "--no-first-run",
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${profile}`,
    "--window-size=1440,960",
    "about:blank",
  ],
  { stdio: "ignore" },
);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function pageSocket() {
  for (let i = 0; i < 50; i++) {
    try {
      const list = await (
        await fetch(`http://127.0.0.1:${port}/json/list`)
      ).json();
      const page = list.find((t) => t.type === "page");
      if (page) return page.webSocketDebuggerUrl;
    } catch {}
    await sleep(200);
  }
  throw new Error("no page");
}
try {
  const ws = new WebSocket(await pageSocket());
  await new Promise((r) => (ws.onopen = r));
  let id = 0;
  const pending = new Map();
  ws.onmessage = (event) => {
    const msg = JSON.parse(event.data);
    if (msg.id && pending.has(msg.id)) {
      pending.get(msg.id)(msg);
      pending.delete(msg.id);
    }
  };
  const send = (method, params = {}) =>
    new Promise((resolve) => {
      const n = ++id;
      pending.set(n, resolve);
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  const evaluate = async (expression) => {
    const out = await send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (out.result?.exceptionDetails)
      throw new Error(JSON.stringify(out.result.exceptionDetails));
    return out.result?.result?.value;
  };
  await send("Page.enable");
  await send("Performance.enable");
  await send("Page.navigate", { url });
  for (let i = 0; i < 120; i++) {
    const ready = await evaluate(
      `document.querySelectorAll('.row').length`,
    ).catch(() => 0);
    if (ready > 0) break;
    await sleep(500);
  }
  await sleep(1500);
  const metrics = async () =>
    Object.fromEntries(
      (await send("Performance.getMetrics")).result.metrics.map((m) => [
        m.name,
        m.value,
      ]),
    );
  if (process.env.PROFILE) {
    await send("Profiler.enable");
    await send("Profiler.start");
  }
  const m0 = await metrics();
  const result = await evaluate(`(async () => {
    const body = document.querySelector('.table .body');
    const rows = () => document.querySelectorAll('.row').length;
    const deltas = [];
    const long = [];
    new PerformanceObserver((list) => { for (const e of list.getEntries()) long.push(e.duration); })
      .observe({ entryTypes: ['longtask'] });
    const total = ${seconds} * 1000;
    const started = performance.now();
    let last = started;
    let scrolled = 0;
    await new Promise((resolve) => {
      function frame(now) {
        deltas.push(now - last); last = now;
        body.scrollTop += ${step}; scrolled = body.scrollTop;
        if (now - started < total) requestAnimationFrame(frame); else resolve();
      }
      requestAnimationFrame(frame);
    });
    deltas.shift();
    deltas.sort((a, b) => a - b);
    const pick = (q) => deltas[Math.min(deltas.length - 1, Math.floor(deltas.length * q))];
    return {
      frames: deltas.length, fps: Math.round(deltas.length / (total / 1000)),
      p50: +pick(0.5).toFixed(1), p95: +pick(0.95).toFixed(1), worst: +deltas[deltas.length - 1].toFixed(1),
      over25ms: deltas.filter((d) => d > 25).length, longTasks: long.length,
      longestTask: long.length ? Math.round(Math.max(...long)) : 0,
      scrolledPx: Math.round(scrolled), domRows: rows(),
      scrollHeight: body.scrollHeight,
    };
  })()`);
  const m1 = await metrics();
  if (process.env.PROFILE) {
    const { result: prof } = await send("Profiler.stop");
    const nodes = new Map(prof.profile.nodes.map((n) => [n.id, n]));
    const self = new Map();
    const dt = prof.profile.timeDeltas;
    prof.profile.samples.forEach((id, i) => {
      const n = nodes.get(id);
      const key = `${n.callFrame.functionName || "(anon)"} ${n.callFrame.url.split("/").slice(-2).join("/")}:${n.callFrame.lineNumber}`;
      self.set(key, (self.get(key) ?? 0) + (dt[i] ?? 0));
    });
    const top = [...self.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, 14)
      .map(([k, v]) => `${Math.round(v / 1000)}ms ${k}`);
    console.log(top.join("\n"));
  }
  const ms = (k) => Math.round((m1[k] - m0[k]) * 1000);
  result.mainThreadMs = {
    script: ms("ScriptDuration"),
    layout: ms("LayoutDuration"),
    style: ms("RecalcStyleDuration"),
    task: ms("TaskDuration"),
  };
  console.log(JSON.stringify(result));
  ws.close();
} finally {
  chrome.kill("SIGTERM");
  await sleep(300);
  rmSync(profile, { recursive: true, force: true });
}
