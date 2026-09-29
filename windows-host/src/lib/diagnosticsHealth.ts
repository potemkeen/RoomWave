import type { AudioState, ReceiverState } from "../types/roomwave";

// UI heuristics, not transport settings or guarantees of audible quality.
export const HEALTH_LIMITS = {
  windowMs: 30_000,
  warmupMs: 10_000,
  sampleIntervalMs: 500,
  staleMs: 5_000,
  minSamples: 8,
  sustainedShare: 0.7,
  minPackets: 200,
  counterIntervals: 3,
  lostPackets: 5,
  lossShare: 0.005,
  latePackets: 5,
  lateShare: 0.01,
  underruns: 3,
  highRttMs: 40,
  supportingRttMs: 15,
  highJitterMs: 8,
  largeJitterBufferMs: 35,
  highLatencyMs: 80,
  slowOutputMs: 60,
  outputLatencyShare: 0.5,
  syncErrorMs: 5,
  groupGapMs: 20,
  groupToleranceMs: 15,
} as const;

// Mirrors the current budget formula only to provide a *possible* explanation.
// Never changes the real group budget; missing inputs disable this inference.
const BUDGET_ESTIMATE = {
  minJitter: 20,
  maxJitter: 50,
  margin: 10,
  min: 40,
  max: 500,
  localMargin: 30,
};

export interface HealthNotice {
  id: string;
  level: "ok" | "info" | "warning";
  title: string;
  detail: string;
  recommendation?: string;
}
export interface ReceiverHealth {
  deviceId: string;
  deviceName: string;
  notices: HealthNotice[];
}

interface Sample {
  at: number;
  packets: number;
  sent: number;
  lost: number | null;
  late: number | null;
  underruns: number | null;
  rtt: number | null;
  jitter: number | null;
  output: number | null;
  queue: number | null;
  latency: number | null;
  sync: number | null;
  syncStatus: string;
  groupCandidate: boolean;
}

const nonnegative = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;
const counter = (value: unknown): number | null => {
  const n = nonnegative(value);
  return n !== null && Number.isSafeInteger(n) ? n : null;
};
const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));
const active = (receiver: ReceiverState) => !["disconnected", "idle"].includes(receiver.status);

function estimatedBudget(receiver: ReceiverState): number | null {
  if (
    receiver.status !== "streaming" ||
    receiver.error ||
    receiver.stages?.outputBackend !== "AAudio" ||
    receiver.transport?.admitted === false
  )
    return null;
  const jitter = nonnegative(receiver.stages.recommendedJitterMs);
  const output = nonnegative(receiver.stages.audioOutputMs);
  const rtt = nonnegative(receiver.rttMs);
  if (jitter === null || output === null || rtt === null) return null;
  return clamp(
    clamp(jitter, BUDGET_ESTIMATE.minJitter, BUDGET_ESTIMATE.maxJitter) +
      output +
      rtt / 2 +
      BUDGET_ESTIMATE.margin,
    BUDGET_ESTIMATE.min,
    BUDGET_ESTIMATE.max,
  );
}

function groupCandidate(audio: AudioState, receiver: ReceiverState): boolean {
  const peers = audio.receivers.filter(active);
  if (peers.length < 2) return false;
  const own = estimatedBudget(receiver);
  const others = peers.filter((peer) => peer.deviceId !== receiver.deviceId).map(estimatedBudget);
  if (audio.localConfig.enabled) {
    const local = nonnegative(audio.localOutput.outputMs);
    others.push(
      local === null || audio.localOutput.error || audio.localOutput.status !== "synced"
        ? null
        : clamp(local + BUDGET_ESTIMATE.localMargin, 10, BUDGET_ESTIMATE.max),
    );
  }
  const target = nonnegative(audio.targetDelayMs);
  if (own === null || target === null || others.some((value) => value === null)) return false;
  const next = Math.max(...(others as number[]));
  return (
    own - next >= HEALTH_LIMITS.groupGapMs &&
    target - next >= HEALTH_LIMITS.groupGapMs &&
    Math.abs(target - own) <= HEALTH_LIMITS.groupToleranceMs
  );
}

function sample(audio: AudioState, receiver: ReceiverState, at: number): Sample | null {
  const packets = counter(receiver.receiverPackets);
  const sent = counter(receiver.packetsSent);
  if (packets === null || sent === null) return null;
  return {
    at,
    packets,
    sent,
    lost: counter(receiver.receiverLost),
    late: counter(receiver.stages?.latePackets),
    underruns: counter(receiver.stages?.underruns),
    rtt: nonnegative(receiver.rttMs),
    jitter: nonnegative(receiver.stages?.jitterMs),
    output: nonnegative(receiver.stages?.audioOutputMs),
    queue: nonnegative(receiver.stages?.jitterBufferMs),
    latency: nonnegative(receiver.latencyMs),
    sync:
      typeof receiver.syncErrorMs === "number" && Number.isFinite(receiver.syncErrorMs)
        ? Math.abs(receiver.syncErrorMs)
        : null,
    syncStatus: receiver.syncStatus,
    groupCandidate: groupCandidate(audio, receiver),
  };
}

function sustained(samples: Sample[], predicate: (sample: Sample) => boolean): boolean {
  return samples.filter(predicate).length / samples.length >= HEALTH_LIMITS.sustainedShare;
}

function growth(samples: Sample[], key: "lost" | "late" | "underruns") {
  let total = 0;
  let intervals = 0;
  for (let i = 1; i < samples.length; i++) {
    const previous = samples[i - 1][key];
    const current = samples[i][key];
    if (previous === null || current === null) return null;
    const delta = current - previous;
    if (delta < 0) return null;
    total += delta;
    if (delta > 0) intervals++;
  }
  return { total, intervals };
}

function insufficient(detail: string): HealthNotice {
  return { id: "insufficient", level: "info", title: "Недостаточно данных для оценки", detail };
}

function classify(
  receiver: ReceiverState,
  samples: Sample[],
  now: number,
  allowGroup: boolean,
): HealthNotice[] {
  if (receiver.error || receiver.status === "error")
    return [
      {
        id: "connection",
        level: "warning",
        title: "Проблема подключения",
        detail: "Данные устройства нельзя считать актуальными.",
        recommendation: "Проверьте сообщение об ошибке ниже и соединение телефона с ПК.",
      },
    ];
  if (receiver.status !== "streaming" || receiver.transport?.admitted === false)
    return [insufficient("Дождитесь подключения и начала передачи звука.")];
  const first = samples[0];
  const last = samples.at(-1);
  if (!first || !last || now - last.at > HEALTH_LIMITS.staleMs)
    return [insufficient("Свежие показатели ещё не поступили. Проверьте, что телефон подключён.")];
  if (
    samples.length < HEALTH_LIMITS.minSamples ||
    last.at - first.at < HEALTH_LIMITS.warmupMs ||
    last.packets - first.packets + Math.max(0, (last.lost ?? 0) - (first.lost ?? 0)) <
      HEALTH_LIMITS.minPackets
  )
    return [insufficient("Собираем показатели: нужно около 10 секунд передачи звука.")];

  const notices: HealthNotice[] = [];
  const add = (id: string, title: string, detail: string, recommendation?: string) =>
    notices.push({ id, level: "warning", title, detail, recommendation });
  const lost = growth(samples, "lost");
  const late = growth(samples, "late");
  const under = growth(samples, "underruns");
  const packets = last.packets - first.packets;
  const repeated = (value: ReturnType<typeof growth>, min: number) =>
    value !== null && value.intervals >= HEALTH_LIMITS.counterIntervals && value.total >= min;
  const losing =
    repeated(lost, HEALTH_LIMITS.lostPackets) &&
    lost!.total / (packets + lost!.total) >= HEALTH_LIMITS.lossShare;
  const arrivingLate =
    repeated(late, HEALTH_LIMITS.latePackets) &&
    late!.total / Math.max(1, packets) >= HEALTH_LIMITS.lateShare;
  const seconds = Math.round((last.at - first.at) / 1000);
  const network = sustained(
    samples,
    (s) =>
      (s.rtt !== null &&
        s.rtt >= HEALTH_LIMITS.highRttMs &&
        s.latency !== null &&
        s.latency >= HEALTH_LIMITS.highLatencyMs) ||
      (s.jitter !== null &&
        s.jitter >= HEALTH_LIMITS.highJitterMs &&
        ((s.rtt !== null && s.rtt >= HEALTH_LIMITS.supportingRttMs) ||
          (s.queue !== null && s.queue >= HEALTH_LIMITS.largeJitterBufferMs) ||
          arrivingLate)),
  );
  if (network)
    add(
      "network",
      "Сеть добавляет задержку",
      "В большинстве последних замеров сеть отвечала медленно или звук поступал неравномерно; это сопровождается увеличенной задержкой или опозданиями.",
      "Попробуйте Wi-Fi 5 ГГц и подойдите ближе к точке доступа.",
    );
  if (
    sustained(
      samples,
      (s) =>
        s.output !== null &&
        s.output >= HEALTH_LIMITS.slowOutputMs &&
        s.latency !== null &&
        s.latency >= HEALTH_LIMITS.highLatencyMs &&
        s.output / s.latency >= HEALTH_LIMITS.outputLatencyShare,
    )
  )
    add(
      "output",
      "Телефон медленно выводит звук",
      "Большая часть задержки приходится на аудиовыход устройства. Это наблюдается в большинстве последних замеров.",
      "Проверьте выбранный аудиовыход. Если используете Bluetooth, сравните со встроенными динамиками.",
    );
  if (arrivingLate)
    add(
      "late",
      "Часть звука приходит слишком поздно",
      `За последние ${seconds} с счётчик опозданий вырос на ${late!.total}; рост повторяется.`,
      network
        ? "Попробуйте улучшить Wi-Fi-соединение."
        : "Сравните работу с включённым экраном и без энергосбережения для RoomWave: причина пока не установлена.",
    );
  if (losing)
    add(
      "loss",
      "Есть пропуски звуковых данных",
      `За последние ${seconds} с счётчик пропусков вырос на ${lost!.total}; это повторяется, а не относится только к началу сеанса.`,
      "Сравните работу рядом с точкой доступа на Wi-Fi 5 ГГц. Причина может быть в доставке или обработке данных.",
    );
  if (repeated(under, HEALTH_LIMITS.underruns))
    add(
      "underruns",
      "Воспроизведению не хватает данных",
      `За последние ${seconds} с отмечено ${under!.total} новых случаев нехватки данных. Возможны прерывания звука.`,
      "Снизьте нагрузку на телефон и сравните работу без энергосбережения для RoomWave.",
    );
  if (sustained(samples, (s) => s.sync !== null && s.sync > HEALTH_LIMITS.syncErrorMs))
    add(
      "sync",
      "Звук отклоняется от общего времени",
      `Отклонение превышает ${HEALTH_LIMITS.syncErrorMs} мс в большинстве последних замеров.`,
      "Если после подключения это не проходит, проверьте остальные предупреждения и переподключите устройство.",
    );
  if (allowGroup && sustained(samples, (s) => s.groupCandidate))
    add(
      "group",
      "Телефон, вероятно, увеличивает задержку всей группы",
      "По показателям сети и аудиовывода ему требуется заметно больший запас, чем другим устройствам. Общий запас близок к этой оценке.",
      "Для проверки временно отключите этот телефон и дождитесь стабилизации задержки остальных. Снижение может быть постепенным.",
    );
  const complete = samples.every((s) =>
    [s.rtt, s.jitter, s.output, s.queue, s.latency, s.sync, s.lost, s.late, s.underruns].every(
      (value) => value !== null,
    ),
  );
  if (!complete)
    notices.push(
      insufficient(
        "Часть показателей отсутствует. Доступные признаки показаны, но полная оценка невозможна.",
      ),
    );
  else if (
    !notices.length &&
    (receiver.syncStatus !== "synced" || sustained(samples, (s) => s.syncStatus !== "synced"))
  )
    notices.push(insufficient("Устройство ещё не подтвердило синхронизацию."));
  else if (!notices.length)
    notices.push({
      id: "healthy",
      level: "ok",
      title: "Всё работает нормально",
      detail: `За последние ${seconds} с устойчивых признаков проблем не обнаружено. Это оценка показателей, а не проверка звука на слух.`,
    });
  return notices;
}

/** Bounded, frontend-only history. Observe real poll results; repeated renders add no evidence. */
export class ReceiverHealthMonitor {
  private history = new Map<string, Sample[]>();
  private audio: AudioState | null = null;

  observe(audio: AudioState, now: number): ReceiverHealth[] {
    this.audio = audio;
    const ids = new Set(audio.receivers.filter(active).map((receiver) => receiver.deviceId));
    for (const id of this.history.keys()) if (!ids.has(id)) this.history.delete(id);
    for (const receiver of audio.receivers.filter(active)) {
      let samples = this.history.get(receiver.deviceId) ?? [];
      const next = sample(audio, receiver, now);
      const previous = samples.at(-1);
      if (
        receiver.status !== "streaming" ||
        receiver.error ||
        receiver.transport?.admitted === false ||
        !next
      ) {
        this.history.delete(receiver.deviceId);
        continue;
      }
      if (
        previous &&
        (now < previous.at ||
          now - previous.at > HEALTH_LIMITS.staleMs ||
          next.sent < previous.sent ||
          next.packets < previous.packets ||
          (["lost", "late", "underruns"] as const).some(
            (key) => next[key] !== null && previous[key] !== null && next[key]! < previous[key]!,
          ))
      )
        samples = [];
      const last = samples.at(-1);
      if (
        !last ||
        (now - last.at >= HEALTH_LIMITS.sampleIntervalMs &&
          (next.packets !== last.packets ||
            next.lost !== last.lost ||
            next.late !== last.late ||
            next.underruns !== last.underruns))
      )
        samples.push(next);
      this.history.set(
        receiver.deviceId,
        samples.filter((s) => now - s.at <= HEALTH_LIMITS.windowMs),
      );
    }
    return this.summarize(now);
  }

  summarize(now: number): ReceiverHealth[] {
    const peers = (this.audio?.receivers ?? []).filter(active);
    const allFresh = peers.every((peer) => {
      const samples = this.history.get(peer.deviceId) ?? [];
      const last = samples.at(-1);
      return (
        last &&
        now - last.at <= HEALTH_LIMITS.staleMs &&
        last.at - samples[0].at >= HEALTH_LIMITS.warmupMs
      );
    });
    return peers.map((receiver) => ({
      deviceId: receiver.deviceId,
      deviceName: receiver.deviceName,
      notices: classify(
        receiver,
        (this.history.get(receiver.deviceId) ?? []).filter(
          (s) => now - s.at <= HEALTH_LIMITS.windowMs,
        ),
        now,
        allFresh && this.audio !== null && groupCandidate(this.audio, receiver),
      ),
    }));
  }
}
