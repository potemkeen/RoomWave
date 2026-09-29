import React from "react";
import {
  getDiagnosticLogState,
  revealDiagnosticLog,
  startDiagnosticLog,
  stopDiagnosticLog,
} from "../services/tauri";

import { MetricRows } from "./MetricRows";
import { DiagnosticsHealth } from "./DiagnosticsHealth";
import type { ReceiverHealth } from "../lib/diagnosticsHealth";

import { formatCount, formatMs, metric, metricText, stateLabel } from "../lib/ui";

import type {
  Action,
  AudioState,
  DiagnosticLogState,
  DiscoverySnapshot,
  VirtualAudio,
} from "../types/roomwave";

interface DiagnosticsPanelProps {
  health: ReceiverHealth[];
  audio: AudioState;
  snapshot: DiscoverySnapshot;
  virtualAudio: VirtualAudio | null;

  logState: DiagnosticLogState | null;

  setLogState: React.Dispatch<React.SetStateAction<DiagnosticLogState | null>>;

  error: string | null;
  busy: Set<string>;

  action: Action;
}

export function DiagnosticsPanel({
  health,
  audio,
  snapshot,
  virtualAudio,
  logState,
  setLogState,
  error,
  busy,
  action,
}: DiagnosticsPanelProps) {
  const outputName = audio.windowsOutputs.find(
    (device) => device.id === audio.localConfig.outputId,
  )?.name;

  return (
    <>
      <DiagnosticsHealth
        receivers={health}
        states={audio.receivers}
        hostError={Boolean(
          error ||
          snapshot.error ||
          audio.captureError ||
          audio.layout.error ||
          audio.localOutput.error ||
          virtualAudio?.error,
        )}
      />
      <section className="diagnostic-block">
        <h3>Аудиодрайвер</h3>

        <p className="help">
          VB-CABLE от VB-Audio — donationware. Если драйвер полезен, автор приветствует оплату
          лицензии или пожертвование. Сайт: https://vb-cable.com · Условия:
          https://vb-audio.com/Services/licensing.htm
        </p>
      </section>

      <section className="diagnostic-block">
        <h3>Запись сеанса</h3>

        <p className="help">
          {logState?.recording
            ? `Записывается · ${logState.samples} замеров · осталось ${Math.max(
                0,
                Math.ceil(((logState.endsAtUnixMs ?? Date.now()) - Date.now()) / 60000),
              )} мин`
            : "Подробная запись выключена"}
        </p>

        <p className="help">
          Для поиска проблем можно записать показатели на 10 минут. Запись остановится
          автоматически. Звук не сохраняется.
        </p>

        <button
          disabled={busy.has("logging")}
          onClick={() =>
            void action("logging", async () => {
              if (logState?.recording) {
                await stopDiagnosticLog();
              } else {
                await startDiagnosticLog();
              }

              setLogState(await getDiagnosticLogState());
            })
          }
        >
          {logState?.recording ? "Остановить запись" : "Записать сеанс · 10 минут"}
        </button>

        {logState?.path && (
          <>
            <button
              disabled={busy.has("logging")}
              onClick={() => void action("logging", revealDiagnosticLog)}
            >
              Показать файл
            </button>

            <pre className="log-path">{logState.path}</pre>
          </>
        )}

        {logState?.error && <pre className="error-detail">{logState.error}</pre>}

        {error && <p className="error-detail">{error}</p>}

        <p className="help">
          Отдельно сохраняется небольшой журнал ошибок: errors.log в папке логов.
        </p>
      </section>

      <p className="panel-description">
        Задержка измеряется от получения PCM хостом до расчётного воспроизведения. Буфер
        виртуального кабеля до захвата и акустическая задержка динамиков в неё не входят; полная
        задержка пока не измерена.
      </p>

      <section className="diagnostic-block">
        <h3>Источник</h3>

        <MetricRows
          rows={[
            ["Устройство", audio.outputName ?? "—"],
            ["Схема звука", audio.layout.name],
            ["Уровень сигнала", `${Math.round(audio.peak * 100)}%`],
            ["Запас перед воспроизведением", formatMs(audio.targetDelayMs)],
          ]}
        />
      </section>

      {audio.localConfig.enabled && (
        <section className="diagnostic-block">
          <h3>Звук на ПК</h3>

          <MetricRows
            rows={[
              ["Устройство", outputName ?? "—"],
              ["Состояние", stateLabel(audio.localOutput.status)],
              ["Задержка", formatMs(audio.localOutput.latencyMs)],
              ["Отклонение синхронизации", formatMs(audio.localOutput.syncErrorMs)],
              ["Буфер вывода", formatMs(audio.localOutput.outputMs)],
            ]}
          />
        </section>
      )}

      {[
        error,
        snapshot.error,
        audio.captureError,
        audio.layout.error,
        audio.localOutput.error,
        virtualAudio?.error,
      ]
        .filter(Boolean)
        .map((item, index) => (
          <pre className="error-detail" key={index}>
            {item}
          </pre>
        ))}

      {audio.hostMetrics && (
        <section className="diagnostic-block">
          <h3>Обработка на ПК</h3>

          <MetricRows
            rows={[
              [
                "Инициализация захвата",
                metricText(audio.hostMetrics, "captureStages", "initPath") ?? "—",
              ],
              [
                "Запрошенный период",
                `${formatCount(metric(audio.hostMetrics, "captureStages", "requestedPeriodFrames"))} кадров / ${formatMs(metric(audio.hostMetrics, "captureStages", "requestedPeriodMs"))}`,
              ],
              [
                "Период захвата Windows",
                `${formatCount(metric(audio.hostMetrics, "captureStages", "engine", "currentPeriodFrames"))} кадров / ${formatMs(metric(audio.hostMetrics, "captureStages", "engine", "currentPeriodMs"))}`,
              ],
              [
                "Буфер захвата WASAPI",
                `${formatCount(metric(audio.hostMetrics, "captureStages", "wasapiBufferFrames"))} кадров / ${formatMs(metric(audio.hostMetrics, "captureStages", "wasapiBufferMs"))}`,
              ],
              [
                "Причина резервного режима",
                metricText(audio.hostMetrics, "captureStages", "fallbackReason") ?? "—",
              ],
              [
                "Чтение → передача, p95",
                formatMs(metric(audio.hostMetrics, "captureStages", "readToPublish", "p95Ms")),
              ],
              [
                "Пропуски в очередях",
                formatCount(metric(audio.hostMetrics, "subscriberQueueDrops")),
              ],
            ]}
          />

          <p className="help">p95 — время, в которое укладываются 95% блоков.</p>
        </section>
      )}
    </>
  );
}
