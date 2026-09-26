import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";

import "./style.css";

import { AudioSettingsPanel } from "./components/AudioSettingsPanel";
import { DevicesView } from "./components/DevicesView";
import { DiagnosticsPanel } from "./components/DiagnosticsPanel";
import { Icon } from "./components/Icon";
import { Panel } from "./components/Panel";
import { SpeakerTest } from "./components/SpeakerTest";

import type {
    AudioState,
    DiagnosticLogState,
    DiscoverySnapshot,
    LocalConfig,
    VirtualAudio,
} from "./types/roomwave";

const initialAudioState: AudioState = {
    outputName: null,
    peak: 0,
    targetDelayMs: 80,

    receivers: [],

    layout: {
        name: "…",
        channelCount: 0,
        channelMask: 0,
        channels: [],
        error: null,
    },

    assignments: {},
    captureError: null,
    testing: null,

    localConfig: {
        enabled: false,
        sourceId: null,
        outputId: null,
        speakers: [],
    },

    windowsOutputs: [],

    localOutput: {
        status: "disabled",
        error: null,
        syncErrorMs: null,
        latencyMs: null,
        outputMs: null,
        unavailable: [],
    },
};

function App() {
    const [
        virtualAudio,
        setVirtualAudio,
    ] =
        React.useState<VirtualAudio | null>(
            null,
        );

    const [logState, setLogState] =
        React.useState<DiagnosticLogState | null>(
            null,
        );

    const [snapshot, setSnapshot] =
        React.useState<DiscoverySnapshot>({
            devices: [],
            discovering: false,
            error: null,
        });

    const [audio, setAudio] =
        React.useState<AudioState>(
            initialAudioState,
        );

    const [loaded, setLoaded] =
        React.useState(false);

    const [panel, setPanel] =
        React.useState<
            "settings" | "diagnostics" | null
        >(null);

    const [view, setView] =
        React.useState<"devices" | "test">(
            "devices",
        );

    const [draft, setDraft] =
        React.useState<LocalConfig | null>(
            null,
        );

    const [error, setError] =
        React.useState<string | null>(null);

    const [flashing, setFlashing] =
        React.useState<number | null>(null);

    const [busy, setBusy] =
        React.useState<Set<string>>(
            new Set(),
        );

    const pending = React.useRef(
        new Set<string>(),
    );

    const config =
        draft ?? audio.localConfig;

    React.useEffect(() => {
        let disposed = false;

        let timer: ReturnType<
            typeof setTimeout
        >;

        async function update() {
            try {
                const [
                    devices,
                    state,
                    log,
                    virtualState,
                ] = await Promise.all([
                    invoke<DiscoverySnapshot>(
                        "get_discovery_state",
                    ),

                    invoke<AudioState>(
                        "get_audio_state",
                    ),

                    invoke<DiagnosticLogState>(
                        "get_diagnostic_log_state",
                    ),

                    invoke<VirtualAudio>(
                        "get_virtual_audio_state",
                    ),
                ]);

                if (!disposed) {
                    setSnapshot(devices);
                    setAudio(state);
                    setLogState(log);
                    setVirtualAudio(
                        virtualState,
                    );
                    setLoaded(true);
                }
            } catch (caughtError) {
                if (!disposed) {
                    setError(
                        String(caughtError),
                    );
                }
            } finally {
                if (!disposed) {
                    timer = setTimeout(
                        update,
                        500,
                    );
                }
            }
        }

        void update();

        return () => {
            disposed = true;
            clearTimeout(timer);
        };
    }, []);

    async function action(
        key: string,
        run: () => Promise<unknown>,
    ) {
        if (pending.current.has(key)) {
            return;
        }

        pending.current.add(key);

        setBusy(
            new Set(pending.current),
        );

        try {
            await run();

            setAudio(
                await invoke<AudioState>(
                    "get_audio_state",
                ),
            );

            setError(null);
        } catch (caughtError) {
            setError(
                String(caughtError),
            );
        } finally {
            pending.current.delete(key);

            setBusy(
                new Set(pending.current),
            );
        }
    }

    const active =
        audio.receivers.filter(
            (receiver) =>
                receiver.status !==
                "disconnected",
        );

    const channels =
        audio.layout.channels;

    const problem = Boolean(
        error ||
        snapshot.error ||
        audio.captureError ||
        audio.layout.error ||
        virtualAudio?.error,
    );

    function openSettings() {
        const next = {
            ...audio.localConfig,
            speakers: [
                ...audio.localConfig.speakers,
            ],
        };

        // Suggest only unambiguous devices;
        // never overwrite a saved or missing endpoint.
        if (!next.sourceId) {
            const matching =
                audio.windowsOutputs.filter(
                    (device) =>
                        device.name ===
                        audio.outputName,
                );

            if (matching.length === 1) {
                next.sourceId =
                    matching[0].id;
            }
        }

        if (!next.outputId) {
            const matching =
                audio.windowsOutputs.filter(
                    (device) =>
                        device.id !==
                        next.sourceId &&
                        !/voicemeeter|virtual|cable|digital|streaming|nvidia|amd|hdmi/i.test(
                            device.name,
                        ),
                );

            if (matching.length === 1) {
                next.outputId =
                    matching[0].id;
            }
        }

        if (!next.speakers.length) {
            next.speakers = channels
                .filter((channel) =>
                    [1, 2, 4].includes(
                        channel.mask,
                    ),
                )
                .map(
                    (channel) =>
                        channel.mask,
                );
        }

        setDraft(next);
        setPanel("settings");
    }

    function closePanel() {
        setPanel(null);
        setDraft(null);
    }

    const saveLocal = (
        next: LocalConfig,
    ) =>
        void action("local", () =>
            invoke(
                "set_local_output",
                {
                    config: next,
                },
            ),
        );

    return (
        <main>
            <header className="app-header">
                <div className="brand">
          <span className="brand-icon">
            <Icon kind="wave" />
          </span>

                    <span>RoomWave</span>
                </div>

                <nav aria-label="Меню">
                    <button
                        className="icon-button"
                        title="Диагностика"
                        aria-label="Открыть диагностику"
                        onClick={() =>
                            setPanel(
                                "diagnostics",
                            )
                        }
                    >
                        <Icon kind="chart" />
                    </button>

                    <button
                        className="icon-button"
                        title="Настройки звука"
                        aria-label="Открыть настройки звука"
                        onClick={openSettings}
                    >
                        <Icon kind="settings" />
                    </button>
                </nav>
            </header>

            <div className="source-strip">
        <span
            className={`status-dot ${
                problem ? "warn" : ""
            }`}
        />

                <span>
          {!loaded
              ? "Подготавливаем звук…"
              : problem
                  ? "Нужно проверить источник звука"
                  : "Источник звука готов"}

                    <small>
            {loaded
                ? audio.layout.name ===
                "Stereo"
                    ? "Стерео"
                    : audio.layout.name ===
                    "Mono"
                        ? "Моно"
                        : audio.layout.name
                : "Определяем доступные каналы"}
          </small>
        </span>

                <button
                    className="text-button"
                    onClick={openSettings}
                >
                    Настроить
                </button>
            </div>

            {problem && (
                <div
                    className="notice"
                    role="alert"
                >
                    Не удалось подготовить звук или
                    обновить устройства. Проверь
                    источник в настройках.{" "}

                    <button
                        className="text-button"
                        onClick={() =>
                            setPanel(
                                "diagnostics",
                            )
                        }
                    >
                        Подробнее
                    </button>
                </div>
            )}

            <div className="section-toolbar">
                <div
                    className="tabs"
                    role="group"
                    aria-label="Раздел"
                >
                    <button
                        aria-pressed={
                            view === "devices"
                        }
                        className={
                            view === "devices"
                                ? "selected"
                                : ""
                        }
                        onClick={() =>
                            setView("devices")
                        }
                    >
                        Устройства
                    </button>

                    <button
                        aria-pressed={
                            view === "test"
                        }
                        className={
                            view === "test"
                                ? "selected"
                                : ""
                        }
                        onClick={() =>
                            setView("test")
                        }
                    >
                        Проверка звука
                    </button>
                </div>

                {view === "devices" && (
                    <span className="count">
            {active.length} подключено
          </span>
                )}
            </div>

            {view === "devices" ? (
                <DevicesView
                    audio={audio}
                    snapshot={snapshot}
                    loaded={loaded}
                    busy={busy}
                    action={action}
                    openSettings={
                        openSettings
                    }
                    openDiagnostics={() =>
                        setPanel(
                            "diagnostics",
                        )
                    }
                    saveLocal={saveLocal}
                />
            ) : (
                <SpeakerTest
                    audio={audio}
                    busy={busy}
                    flashing={flashing}
                    setFlashing={
                        setFlashing
                    }
                    action={action}
                />
            )}

            {panel && (
                <Panel
                    title={
                        panel === "settings"
                            ? "Настройки звука"
                            : "Диагностика"
                    }
                    onClose={closePanel}
                >
                    {panel ===
                    "settings" ? (
                        <AudioSettingsPanel
                            audio={audio}
                            virtualAudio={
                                virtualAudio
                            }
                            config={config}
                            busy={busy}
                            error={error}
                            loaded={loaded}
                            draft={draft}
                            setDraft={setDraft}
                            action={action}
                            onCancel={
                                closePanel
                            }
                        />
                    ) : (
                        <DiagnosticsPanel
                            audio={audio}
                            snapshot={snapshot}
                            virtualAudio={
                                virtualAudio
                            }
                            logState={
                                logState
                            }
                            setLogState={
                                setLogState
                            }
                            error={error}
                            busy={busy}
                            action={action}
                        />
                    )}
                </Panel>
            )}
        </main>
    );
}

ReactDOM.createRoot(
    document.getElementById("root")!,
).render(
    <React.StrictMode>
        <App />
    </React.StrictMode>,
);
