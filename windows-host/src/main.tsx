import React from "react";
import ReactDOM from "react-dom/client";

import "./style.css";

import { AudioSettingsPanel } from "./components/AudioSettingsPanel";
import { DevicesView } from "./components/DevicesView";
import { DiagnosticsPanel } from "./components/DiagnosticsPanel";
import { Icon } from "./components/Icon";
import { Panel } from "./components/Panel";
import { SpeakerTest } from "./components/SpeakerTest";
import { useRoomWaveState } from "./hooks/useRoomWaveState";
import { setLocalOutput } from "./services/tauri";

import type { LocalConfig } from "./types/roomwave";

function App() {
  const { virtualAudio, logState, setLogState, snapshot, audio, loaded, error, busy, action } =
    useRoomWaveState();

  const [panel, setPanel] = React.useState<"settings" | "diagnostics" | null>(null);

  const [view, setView] = React.useState<"devices" | "test">("devices");

  const [draft, setDraft] = React.useState<LocalConfig | null>(null);

  const [flashing, setFlashing] = React.useState<number | null>(null);

  const config = draft ?? audio.localConfig;

  const active = audio.receivers.filter((receiver) => receiver.status !== "disconnected");

  const channels = audio.layout.channels;

  const problem = Boolean(
    error || snapshot.error || audio.captureError || audio.layout.error || virtualAudio?.error,
  );

  function openSettings() {
    const next = {
      ...audio.localConfig,
      speakers: [...audio.localConfig.speakers],
    };

    // Suggest only unambiguous devices;
    // never overwrite a saved or missing endpoint.
    if (!next.sourceId) {
      const matching = audio.windowsOutputs.filter((device) => device.name === audio.outputName);

      if (matching.length === 1) {
        next.sourceId = matching[0].id;
      }
    }

    if (!next.outputId) {
      const matching = audio.windowsOutputs.filter(
        (device) =>
          device.id !== next.sourceId &&
          !/voicemeeter|virtual|cable|digital|streaming|nvidia|amd|hdmi/i.test(device.name),
      );

      if (matching.length === 1) {
        next.outputId = matching[0].id;
      }
    }

    if (!next.speakers.length) {
      next.speakers = channels
        .filter((channel) => [1, 2, 4].includes(channel.mask))
        .map((channel) => channel.mask);
    }

    setDraft(next);
    setPanel("settings");
  }

  function closePanel() {
    setPanel(null);
    setDraft(null);
  }

  const saveLocal = (next: LocalConfig) => void action("local", () => setLocalOutput(next));

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
            onClick={() => setPanel("diagnostics")}
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
        <span className={`status-dot ${problem ? "warn" : ""}`} />

        <span>
          {!loaded
            ? "Подготавливаем звук…"
            : problem
              ? "Нужно проверить источник звука"
              : "Источник звука готов"}

          <small>
            {loaded
              ? audio.layout.name === "Stereo"
                ? "Стерео"
                : audio.layout.name === "Mono"
                  ? "Моно"
                  : audio.layout.name
              : "Определяем доступные каналы"}
          </small>
        </span>

        <button className="text-button" onClick={openSettings}>
          Настроить
        </button>
      </div>

      {problem && (
        <div className="notice" role="alert">
          Не удалось подготовить звук или обновить устройства. Проверь источник в настройках.{" "}
          <button className="text-button" onClick={() => setPanel("diagnostics")}>
            Подробнее
          </button>
        </div>
      )}

      <div className="section-toolbar">
        <div className="tabs" role="group" aria-label="Раздел">
          <button
            aria-pressed={view === "devices"}
            className={view === "devices" ? "selected" : ""}
            onClick={() => setView("devices")}
          >
            Устройства
          </button>

          <button
            aria-pressed={view === "test"}
            className={view === "test" ? "selected" : ""}
            onClick={() => setView("test")}
          >
            Проверка звука
          </button>
        </div>

        {view === "devices" && <span className="count">{active.length} подключено</span>}
      </div>

      {view === "devices" ? (
        <DevicesView
          audio={audio}
          snapshot={snapshot}
          loaded={loaded}
          busy={busy}
          action={action}
          openSettings={openSettings}
          openDiagnostics={() => setPanel("diagnostics")}
          saveLocal={saveLocal}
        />
      ) : (
        <SpeakerTest
          audio={audio}
          busy={busy}
          flashing={flashing}
          setFlashing={setFlashing}
          action={action}
        />
      )}

      {panel && (
        <Panel
          title={panel === "settings" ? "Настройки звука" : "Диагностика"}
          onClose={closePanel}
        >
          {panel === "settings" ? (
            <AudioSettingsPanel
              audio={audio}
              virtualAudio={virtualAudio}
              config={config}
              busy={busy}
              error={error}
              loaded={loaded}
              draft={draft}
              setDraft={setDraft}
              action={action}
              onCancel={closePanel}
            />
          ) : (
            <DiagnosticsPanel
              audio={audio}
              snapshot={snapshot}
              virtualAudio={virtualAudio}
              logState={logState}
              setLogState={setLogState}
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

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
