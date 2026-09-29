import type { Speaker } from "../types/roomwave";

export const speakerPosition: Record<string, [number, number]> = {
  FL: [1, 1],
  FR: [1, 7],
  FC: [1, 4],
  LFE: [5, 4],
  BL: [5, 1],
  BR: [5, 7],
  SL: [3, 1],
  SR: [3, 7],
  FLC: [1, 3],
  FRC: [1, 5],
  BC: [5, 3],
};

const speakerNames: Record<string, string> = {
  FL: "Передний левый",
  FR: "Передний правый",
  FC: "Центр",
  LFE: "Сабвуфер",
  BL: "Задний левый",
  BR: "Задний правый",
  SL: "Боковой левый",
  SR: "Боковой правый",
  FLC: "Передний левый центральный",
  FRC: "Передний правый центральный",
  BC: "Задний центральный",
};

export const speakerTitle = (speaker: Speaker) => speakerNames[speaker.code] ?? speaker.name;

export const formatMs = (value: number | null | undefined) =>
  value == null ? "—" : `${value.toFixed(1)} мс`;

export const formatCount = (value: number | null) =>
  value == null ? "—" : value.toLocaleString("ru-RU");

// An explicit scalar allowlist keeps new backend diagnostics in logs,
// not in the UI.
export function metric(source: unknown, ...path: string[]): number | null {
  let value = source;

  for (const key of path) {
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      return null;
    }

    value = (value as Record<string, unknown>)[key];
  }

  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function metricText(source: unknown, ...path: string[]): string | null {
  let value = source;
  for (const key of path) {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    value = (value as Record<string, unknown>)[key];
  }
  return typeof value === "string" ? value : null;
}

export const stateLabel = (state: string) =>
  ({
    synced: "Синхронизировано",
    aligning: "Синхронизация",
    buffering: "Ожидание звука",
    connecting: "Подключение",
    streaming: "Подключено",
    error: "Ошибка",
    idle: "Не активно",
    preparing: "Подготовка",
    warming: "Подготовка",
    audioClock: "Подготовка аудиовывода",
    disconnected: "Отключено",
    waiting: "Ожидание",
  })[state] ?? "—";
