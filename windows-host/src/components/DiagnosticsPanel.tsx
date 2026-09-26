import React from "react";
import { invoke } from "@tauri-apps/api/core";

import { MetricRows } from "./MetricRows";

import {
    formatCount,
    formatMs,
    metric,
    stateLabel,
} from "../lib/ui";

import type {
    Action,
    AudioState,
    DiagnosticLogState,
    DiscoverySnapshot,
    VirtualAudio,
} from "../types/roomwave";

interface DiagnosticsPanelProps {
    audio: AudioState;
    snapshot: DiscoverySnapshot;
    virtualAudio: VirtualAudio | null;

    logState: DiagnosticLogState | null;

    setLogState: React.Dispatch<
        React.SetStateAction<
            DiagnosticLogState | null
        >
    >;

    error: string | null;
    busy: Set<string>;

    action: Action;
}

export function DiagnosticsPanel({
                                     audio,
                                     snapshot,
                                     virtualAudio,
                                     logState,
                                     setLogState,
                                     error,
                                     busy,
                                     action,
                                 }: DiagnosticsPanelProps) {
    const outputName =
        audio.windowsOutputs.find(
            (device) =>
                device.id ===
                audio.localConfig.outputId,
        )?.name;

    return (
        <>
            <section className="diagnostic-block">
                <h3>Аудиодрайвер</h3>

                <p className="help">
                    VB-CABLE от VB-Audio — donationware.
                    Если драйвер полезен, автор
                    приветствует оплату лицензии или
                    пожертвование. Сайт:
                    https://vb-cable.com · Условия:
                    https://vb-audio.com/Services/licensing.htm
                </p>
            </section>

            <section className="diagnostic-block">
                <h3>Запись сеанса</h3>

                <p className="help">
                    {logState?.recording
                        ? `Записывается · ${logState.samples} замеров · осталось ${Math.max(
                            0,
                            Math.ceil(
                                ((logState.endsAtUnixMs ??
                                        Date.now()) -
                                    Date.now()) /
                                60000,
                            ),
                        )} мин`
                        : "Подробная запись выключена"}
                </p>

                <p className="help">
                    Для поиска проблем можно записать
                    показатели на 10 минут. Запись
                    остановится автоматически. Звук не
                    сохраняется.
                </p>

                <button
                    disabled={busy.has("logging")}
                    onClick={() =>
                        void action(
                            "logging",
                            async () => {
                                await invoke(
                                    logState?.recording
                                        ? "stop_diagnostic_log"
                                        : "start_diagnostic_log",
                                );

                                setLogState(
                                    await invoke<DiagnosticLogState>(
                                        "get_diagnostic_log_state",
                                    ),
                                );
                            },
                        )
                    }
                >
                    {logState?.recording
                        ? "Остановить запись"
                        : "Записать сеанс · 10 минут"}
                </button>

                {logState?.path && (
                    <>
                        <button
                            disabled={
                                busy.has("logging")
                            }
                            onClick={() =>
                                void action(
                                    "logging",
                                    () =>
                                        invoke(
                                            "reveal_diagnostic_log",
                                        ),
                                )
                            }
                        >
                            Показать файл
                        </button>

                        <pre className="log-path">
              {logState.path}
            </pre>
                    </>
                )}

                {logState?.error && (
                    <pre className="error-detail">
            {logState.error}
          </pre>
                )}

                {error && (
                    <p className="error-detail">
                        {error}
                    </p>
                )}

                <p className="help">
                    Отдельно сохраняется небольшой
                    журнал ошибок: errors.log в папке
                    логов.
                </p>
            </section>

            <p className="panel-description">
                Задержка измеряется от получения PCM
                хостом до расчётного воспроизведения.
                Буфер виртуального кабеля до захвата и
                акустическая задержка динамиков в неё
                не входят; полная задержка пока не
                измерена.
            </p>

            <section className="diagnostic-block">
                <h3>Источник</h3>

                <MetricRows
                    rows={[
                        [
                            "Устройство",
                            audio.outputName ?? "—",
                        ],
                        [
                            "Схема звука",
                            audio.layout.name,
                        ],
                        [
                            "Уровень сигнала",
                            `${Math.round(audio.peak * 100)}%`,
                        ],
                        [
                            "Запас перед воспроизведением",
                            formatMs(
                                audio.targetDelayMs,
                            ),
                        ],
                    ]}
                />
            </section>

            {audio.localConfig.enabled && (
                <section className="diagnostic-block">
                    <h3>Звук на ПК</h3>

                    <MetricRows
                        rows={[
                            [
                                "Устройство",
                                outputName ?? "—",
                            ],
                            [
                                "Состояние",
                                stateLabel(
                                    audio.localOutput.status,
                                ),
                            ],
                            [
                                "Задержка",
                                formatMs(
                                    audio.localOutput
                                        .latencyMs,
                                ),
                            ],
                            [
                                "Отклонение синхронизации",
                                formatMs(
                                    audio.localOutput
                                        .syncErrorMs,
                                ),
                            ],
                            [
                                "Буфер вывода",
                                formatMs(
                                    audio.localOutput
                                        .outputMs,
                                ),
                            ],
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
                    <pre
                        className="error-detail"
                        key={index}
                    >
            {item}
          </pre>
                ))}

            {audio.hostMetrics && (
                <section className="diagnostic-block">
                    <h3>Обработка на ПК</h3>

                    <MetricRows
                        rows={[
                            [
                                "Период захвата Windows",
                                formatMs(
                                    metric(
                                        audio.hostMetrics,
                                        "captureStages",
                                        "engine",
                                        "currentPeriodMs",
                                    ),
                                ),
                            ],
                            [
                                "Чтение → передача, p95",
                                formatMs(
                                    metric(
                                        audio.hostMetrics,
                                        "captureStages",
                                        "readToPublish",
                                        "p95Ms",
                                    ),
                                ),
                            ],
                            [
                                "Пропуски в очередях",
                                formatCount(
                                    metric(
                                        audio.hostMetrics,
                                        "subscriberQueueDrops",
                                    ),
                                ),
                            ],
                        ]}
                    />

                    <p className="help">
                        p95 — время, в которое
                        укладываются 95% блоков.
                    </p>
                </section>
            )}

            {audio.receivers.map((receiver) => (
                <section
                    className="diagnostic-block"
                    key={receiver.deviceId}
                >
                    <h3>{receiver.deviceName}</h3>

                    <MetricRows
                        rows={[
                            [
                                "Соединение",
                                receiver.error
                                    ? "Ошибка"
                                    : stateLabel(
                                        receiver.status,
                                    ),
                            ],
                            [
                                "Синхронизация",
                                stateLabel(
                                    receiver.syncStatus,
                                ),
                            ],
                            [
                                "Задержка",
                                formatMs(
                                    receiver.latencyMs,
                                ),
                            ],
                            [
                                "Сеть (RTT)",
                                formatMs(receiver.rttMs),
                            ],
                            [
                                "Колебания задержки сети",
                                formatMs(
                                    metric(
                                        receiver.stages,
                                        "jitterMs",
                                    ),
                                ),
                            ],
                            [
                                "Отклонение синхронизации",
                                formatMs(
                                    receiver.syncErrorMs,
                                ),
                            ],
                            [
                                "Буфер перед воспроизведением",
                                formatMs(
                                    metric(
                                        receiver.stages,
                                        "jitterBufferMs",
                                    ),
                                ),
                            ],
                            [
                                "Буфер аудиовывода",
                                formatMs(
                                    metric(
                                        receiver.stages,
                                        "audioOutputMs",
                                    ),
                                ),
                            ],
                            [
                                "Пропущено пакетов",
                                formatCount(
                                    receiver.receiverLost,
                                ),
                            ],
                            [
                                "Опоздало пакетов",
                                formatCount(
                                    metric(
                                        receiver.stages,
                                        "latePackets",
                                    ),
                                ),
                            ],
                            [
                                "Нехватка данных для вывода",
                                formatCount(
                                    metric(
                                        receiver.stages,
                                        "underruns",
                                    ),
                                ),
                            ],
                        ]}
                    />

                    <p className="help">
                        Счётчики — с начала подключения.
                        Отсутствующие данные обозначены
                        «—».
                    </p>

                    {receiver.error && (
                        <pre className="error-detail">
              {receiver.error}
            </pre>
                    )}
                </section>
            ))}

            {audio.receivers.length === 0 && (
                <p className="help">
                    Метрики телефонов появятся после
                    подключения.
                </p>
            )}
        </>
    );
}
