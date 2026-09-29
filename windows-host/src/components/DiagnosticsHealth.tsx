import type { ReceiverHealth } from "../lib/diagnosticsHealth";
import type { ReceiverState } from "../types/roomwave";
import { formatCount, formatMs, metric, stateLabel } from "../lib/ui";
import { MetricRows } from "./MetricRows";

export function DiagnosticsHealth({
  receivers,
  states,
  hostError,
}: {
  receivers: ReceiverHealth[];
  states: ReceiverState[];
  hostError: boolean;
}) {
  return (
    <section className="diagnostic-block diagnostic-health" aria-labelledby="system-health-title">
      <h3 id="system-health-title">Состояние системы</h3>
      <p className="help">
        Оценка за последние 30 секунд. После подключения нужно около 10 секунд наблюдения.
      </p>
      {hostError && <p className="inline-warning">Есть ошибки на ПК. Проверьте сообщения ниже.</p>}
      {states.length === 0 && <p className="help">Метрики телефонов появятся после подключения.</p>}
      {states.map((receiver) => {
        const notices =
          receivers.find((item) => item.deviceId === receiver.deviceId)?.notices ?? [];
        const warnings = notices.filter((notice) => notice.level === "warning");
        const level = warnings.length
          ? "warning"
          : notices.some((notice) => notice.level === "ok")
            ? "ok"
            : "info";
        const title = warnings.length
          ? "Есть замечания"
          : (notices[0]?.title ??
            (receiver.status === "disconnected" ? "Телефон отключён" : "Ожидаем данные"));
        const summary = warnings.length
          ? warnings.map((notice) => notice.title).join(" · ")
          : (notices[0]?.detail ?? "Подробные показатели доступны ниже.");
        return (
          <div className="health-receiver" key={receiver.deviceId}>
            <h4>{receiver.deviceName}</h4>
            <div className={`health-card health-${level}`}>
              <div className="health-summary">
                <strong>
                  <span aria-hidden="true">
                    {level === "ok" ? "✓ " : level === "warning" ? "! " : "… "}
                  </span>
                  {title}
                </strong>
                <p title={summary}>{summary}</p>
              </div>
              <details className="health-details">
                <summary>Подробности и показатели</summary>
                <div className="health-details-content">
                  {notices.map((notice) => (
                    <div className="health-explanation" key={notice.id}>
                      <strong>{notice.title}</strong>
                      <p>{notice.detail}</p>
                      {notice.recommendation && (
                        <p className="health-advice">Что можно сделать: {notice.recommendation}</p>
                      )}
                    </div>
                  ))}
                  <MetricRows
                    rows={[
                      ["Соединение", receiver.error ? "Ошибка" : stateLabel(receiver.status)],
                      ["Синхронизация", stateLabel(receiver.syncStatus)],
                      ["Задержка", formatMs(receiver.latencyMs)],
                      ["Сеть (RTT)", formatMs(receiver.rttMs)],
                      ["Колебания задержки сети", formatMs(metric(receiver.stages, "jitterMs"))],
                      ["Отклонение синхронизации", formatMs(receiver.syncErrorMs)],
                      [
                        "Буфер перед воспроизведением",
                        formatMs(metric(receiver.stages, "jitterBufferMs")),
                      ],
                      ["Буфер аудиовывода", formatMs(metric(receiver.stages, "audioOutputMs"))],
                      ["Пропущено пакетов", formatCount(receiver.receiverLost)],
                      ["Опоздало пакетов", formatCount(metric(receiver.stages, "latePackets"))],
                      [
                        "Нехватка данных для вывода",
                        formatCount(metric(receiver.stages, "underruns")),
                      ],
                    ]}
                  />
                  <p className="help">
                    Счётчики — с начала подключения. Отсутствующие данные обозначены «—».
                  </p>
                  {receiver.error && <pre className="error-detail">{receiver.error}</pre>}
                </div>
              </details>
            </div>
          </div>
        );
      })}
    </section>
  );
}
