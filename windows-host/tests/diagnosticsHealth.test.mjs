import test from "node:test";
import assert from "node:assert/strict";
import { ReceiverHealthMonitor, HEALTH_LIMITS } from "../.test-dist/lib/diagnosticsHealth.js";

function receiver(i, overrides = {}) {
  return {
    deviceId: "samsung",
    deviceName: "Samsung",
    status: "streaming",
    packetsSent: 100 + i * 200,
    receiverPackets: 100 + i * 200,
    receiverLost: 0,
    latencyMs: 60,
    rttMs: 6,
    syncErrorMs: 0.4,
    syncStatus: "synced",
    error: null,
    transport: { admitted: true },
    ...overrides,
    stages: {
      outputBackend: "AAudio",
      recommendedJitterMs: 20,
      jitterMs: 0.5,
      audioOutputMs: 28,
      jitterBufferMs: 26,
      latePackets: 0,
      underruns: 0,
      ...overrides.stages,
    },
  };
}
function audio(receivers, overrides = {}) {
  return {
    receivers,
    targetDelayMs: 61,
    localConfig: { enabled: false },
    localOutput: { status: "disabled", outputMs: null, error: null },
    ...overrides,
  };
}
function run(
  make = (i) => audio([receiver(i)]),
  seconds = 30,
  monitor = new ReceiverHealthMonitor(),
) {
  let result;
  for (let i = 0; i <= seconds; i++) result = monitor.observe(make(i), i * 1000);
  return { monitor, result };
}
const ids = (result, id = "samsung") =>
  result.find((r) => r.deviceId === id)?.notices.map((n) => n.id) ?? [];

test("healthy Samsung-like receiver has no warnings", () => {
  assert.deepEqual(ids(run().result), ["healthy"]);
});
test("high network jitter with repeated late packets", () => {
  const { result } = run((i) =>
    audio([
      receiver(i, {
        rttMs: 25,
        latencyMs: 110,
        stages: { jitterMs: 15, jitterBufferMs: 45, latePackets: i * 5 },
      }),
    ]),
  );
  assert.ok(ids(result).includes("network"));
  assert.ok(ids(result).includes("late"));
  assert.ok(!ids(result).includes("output"));
});
test("persistently high RTT needs supporting latency", () => {
  assert.ok(
    ids(run((i) => audio([receiver(i, { rttMs: 60, latencyMs: 120 })])).result).includes("network"),
  );
});
test("slow Android output is distinguished from a good network", () => {
  const { result } = run((i) =>
    audio([receiver(i, { latencyMs: 125, stages: { audioOutputMs: 85 } })]),
  );
  assert.deepEqual(ids(result), ["output"]);
});
test("packet loss is based on recent growth, not lifetime totals", () => {
  assert.ok(
    ids(run((i) => audio([receiver(i, { receiverLost: 5000 + i * 3 })])).result).includes("loss"),
  );
  assert.deepEqual(
    ids(
      run((i) =>
        audio([receiver(i, { receiverLost: 5000, stages: { latePackets: 8000, underruns: 500 } })]),
      ).result,
    ),
    ["healthy"],
  );
});
test("continued loss reports remain actionable when no audio packets arrive", () => {
  const { result } = run((i) =>
    audio([receiver(i, { receiverPackets: 100, receiverLost: i * 200 })]),
  );
  assert.ok(ids(result).includes("loss"));
  assert.ok(!ids(result).includes("healthy"));
});

test("repeated underruns are reported even on a good network", () => {
  assert.ok(
    ids(run((i) => audio([receiver(i, { stages: { underruns: i } })])).result).includes(
      "underruns",
    ),
  );
});
test("sync uses absolute deviation sustained over time", () => {
  assert.ok(ids(run((i) => audio([receiver(i, { syncErrorMs: -9 })])).result).includes("sync"));
});
function group(i, overrides = {}) {
  return audio(
    [
      receiver(i),
      receiver(i, {
        deviceId: "xiaomi",
        deviceName: "Xiaomi",
        latencyMs: 125,
        stages: { audioOutputMs: 85 },
      }),
    ],
    { targetDelayMs: 125, ...overrides },
  );
}
test("slower receiver plausibly raises group budget", () => {
  const { result } = run((i) => group(i));
  assert.ok(ids(result, "xiaomi").includes("group"));
  assert.ok(!ids(result).includes("group"));
  assert.match(result[1].notices.find((n) => n.id === "group").title, /вероятно/);
});
test("no group blame for one receiver, tied budgets or a higher local-output budget", () => {
  const one = run((i) =>
    audio([receiver(i, { latencyMs: 125, stages: { audioOutputMs: 85 } })], { targetDelayMs: 125 }),
  );
  assert.ok(!ids(one.result).includes("group"));
  const tied = run((i) => audio([receiver(i), receiver(i, { deviceId: "other" })]));
  assert.ok(tied.result.every((r) => !r.notices.some((n) => n.id === "group")));
  const local = run((i) =>
    group(i, {
      localConfig: { enabled: true },
      localOutput: { status: "synced", outputMs: 110, error: null },
    }),
  );
  assert.ok(!ids(local.result, "xiaomi").includes("group"));
});
test("unknown peer budget or lingering high group target prevents attribution", () => {
  const unknown = run((i) => {
    const state = group(i);
    delete state.receivers[0].stages.recommendedJitterMs;
    return state;
  });
  assert.ok(!ids(unknown.result, "xiaomi").includes("group"));
  assert.ok(!ids(run((i) => group(i, { targetDelayMs: 200 })).result, "xiaomi").includes("group"));
});
test("group attribution clears on peer removal and requires fresh peer data", () => {
  const { monitor } = run((i) => group(i));
  const solo = group(31);
  solo.receivers.shift();
  assert.ok(!ids(monitor.observe(solo, 31_000), "xiaomi").includes("group"));
  const stale = run((i) => {
    const state = group(i);
    state.receivers[0] = receiver(0);
    return state;
  });
  assert.ok(!ids(stale.result, "xiaomi").includes("group"));
});
test("missing, nonfinite and invalid metrics cannot produce healthy", () => {
  const { result } = run((i) =>
    audio([
      receiver(i, {
        rttMs: null,
        stages: { audioOutputMs: NaN, jitterMs: Infinity, latePackets: -1 },
      }),
    ]),
  );
  assert.deepEqual(ids(result), ["insufficient"]);
});
test("isolated spikes and a single counter jump do not trigger alarms", () => {
  const { result } = run((i) =>
    audio([
      receiver(i, {
        rttMs: i === 15 ? 300 : 6,
        syncErrorMs: i === 15 ? 80 : 0.4,
        receiverLost: i >= 15 ? 100 : 0,
        stages: {
          jitterMs: i === 15 ? 60 : 0.5,
          latePackets: i >= 15 ? 100 : 0,
          underruns: i >= 15 ? 20 : 0,
        },
      }),
    ]),
  );
  assert.deepEqual(ids(result), ["healthy"]);
});
test("startup and reconnect reset the evidence window", () => {
  assert.deepEqual(ids(run(undefined, 5).result), ["insufficient"]);
  const { monitor } = run();
  const result = monitor.observe(audio([receiver(0)]), 31_000);
  assert.deepEqual(ids(result), ["insufficient"]);
  assert.deepEqual(monitor.observe(audio([]), 32_000), []);
  assert.deepEqual(ids(monitor.observe(audio([receiver(50)]), 33_000)), ["insufficient"]);
});
test("warming synchronization and absent native metrics stay unassessed", () => {
  assert.deepEqual(ids(run((i) => audio([receiver(i, { syncStatus: "warming" })])).result), [
    "insufficient",
  ]);
  assert.deepEqual(
    ids(
      run((i) => {
        const peer = receiver(i);
        peer.stages = null;
        return audio([peer]);
      }).result,
    ),
    ["insufficient"],
  );
});
test("a reset of an individual counter restarts observation", () => {
  const { monitor } = run((i) => audio([receiver(i, { stages: { underruns: i } })]));
  assert.deepEqual(ids(monitor.observe(audio([receiver(31)]), 31_000)), ["insufficient"]);
});
test("duplicate polls and stale telemetry cannot stay healthy", () => {
  const { monitor } = run();
  const same = audio([receiver(30)]);
  for (let i = 31; i <= 34; i++) monitor.observe(same, i * 1000);
  assert.deepEqual(ids(monitor.summarize(30_000 + HEALTH_LIMITS.staleMs + 1)), ["insufficient"]);
});
test("warnings expire after a healthy observation window", () => {
  const { monitor } = run((i) => audio([receiver(i, { receiverLost: i * 3 })]));
  let result;
  for (let i = 31; i <= 65; i++)
    result = monitor.observe(audio([receiver(i, { receiverLost: 90 })]), i * 1000);
  assert.deepEqual(ids(result), ["healthy"]);
});
test("disconnected receivers are excluded and connection errors are explicit", () => {
  const monitor = new ReceiverHealthMonitor();
  assert.deepEqual(monitor.observe(audio([receiver(0, { status: "disconnected" })]), 0), []);
  assert.deepEqual(
    ids(monitor.observe(audio([receiver(0, { status: "error", error: "timeout" })]), 1000)),
    ["connection"],
  );
});
