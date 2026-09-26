import React from "react";
import { setLocalOutput } from "../services/tauri";

import { speakerTitle } from "../lib/ui";

import type { Action, AudioState, LocalConfig, VirtualAudio } from "../types/roomwave";

interface AudioSettingsPanelProps {
  audio: AudioState;
  virtualAudio: VirtualAudio | null;

  config: LocalConfig;

  busy: Set<string>;
  error: string | null;
  loaded: boolean;

  draft: LocalConfig | null;

  setDraft: React.Dispatch<React.SetStateAction<LocalConfig | null>>;

  action: Action;

  onCancel: () => void;
}

export function AudioSettingsPanel({
  audio,
  virtualAudio,
  config,
  busy,
  error,
  loaded,
  draft,
  setDraft,
  action,
  onCancel,
}: AudioSettingsPanelProps) {
  const channels = audio.layout.channels;

  const invalidConfig =
    config.enabled &&
    (!config.sourceId ||
      !config.outputId ||
      config.sourceId === config.outputId ||
      config.speakers.length === 0);

  return (
    <>
      {virtualAudio?.error && <p className="error">{virtualAudio.error}</p>}

      <p className="panel-description">
        {virtualAudio?.endpointId
          ? "Источник RoomWave настроен автоматически. Выбери выход ПК и назначь каналы."
          : "Выбери источник звука и выход ПК. Для автоматической настройки установи драйвер RoomWave Virtual Speakers."}
      </p>

      <label className="field">
        Источник звука
        <select
          value={config.sourceId ?? ""}
          disabled={busy.has("local") || Boolean(virtualAudio?.endpointId)}
          onChange={(event) =>
            setDraft({
              ...config,
              sourceId: event.target.value || null,
            })
          }
        >
          <option value="">Как в Windows</option>

          {config.sourceId &&
            !audio.windowsOutputs.some((device) => device.id === config.sourceId) && (
              <option value={config.sourceId}>Сохранённый источник недоступен</option>
            )}

          {audio.windowsOutputs.map((device) => (
            <option key={device.id} value={device.id}>
              {device.name}
            </option>
          ))}
        </select>
      </label>

      <p className="help">
        {virtualAudio?.endpointId
          ? "Windows переключается на RoomWave при запуске. После закрытия прежний выход восстанавливается, если ты не переключил его вручную."
          : "Для объёмного звука нужен источник с каналами 5.1/7.1. В Windows или плеере должен быть выбран тот же выход."}
      </p>

      <label className="field">
        Наушники или колонки ПК
        <select
          value={config.outputId ?? ""}
          disabled={busy.has("local")}
          onChange={(event) =>
            setDraft({
              ...config,
              outputId: event.target.value || null,
            })
          }
        >
          <option value="">Выбери устройство</option>

          {config.outputId &&
            !audio.windowsOutputs.some((device) => device.id === config.outputId) && (
              <option value={config.outputId}>Сохранённый выход недоступен</option>
            )}

          {audio.windowsOutputs
            .filter(
              (device) =>
                device.id !== virtualAudio?.endpointId &&
                (device.id !== config.sourceId || device.id === config.outputId),
            )
            .map((device) => (
              <option key={device.id} value={device.id}>
                {device.name}
              </option>
            ))}
        </select>
      </label>

      <p className="help">
        Для наушников оставь схему Stereo в Windows: центральный канал будет слышен с обеих сторон.
      </p>

      <label className="check-row">
        <input
          type="checkbox"
          checked={config.enabled}
          disabled={busy.has("local")}
          onChange={(event) =>
            setDraft({
              ...config,
              enabled: event.target.checked,
            })
          }
        />
        Воспроизводить выбранные каналы на ПК
      </label>

      <fieldset>
        <legend>Каналы компьютера</legend>

        <div className="channel-chips">
          {channels.map((channel) => (
            <label
              key={channel.mask}
              className={config.speakers.includes(channel.mask) ? "chosen" : ""}
            >
              <input
                type="checkbox"
                disabled={busy.has("local")}
                checked={config.speakers.includes(channel.mask)}
                onChange={(event) =>
                  setDraft({
                    ...config,
                    speakers: event.target.checked
                      ? [...config.speakers, channel.mask]
                      : config.speakers.filter((speaker) => speaker !== channel.mask),
                  })
                }
              />

              {speakerTitle(channel)}
            </label>
          ))}

          {config.speakers
            .filter((speaker) => !channels.some((channel) => channel.mask === speaker))
            .map((speaker) => (
              <label key={speaker}>
                <input
                  type="checkbox"
                  checked
                  disabled={busy.has("local")}
                  onChange={() =>
                    setDraft({
                      ...config,
                      speakers: config.speakers.filter((value) => value !== speaker),
                    })
                  }
                />
                Недоступный канал ({speaker})
              </label>
            ))}
        </div>
      </fieldset>

      {config.sourceId !== audio.localConfig.sourceId && (
        <p className="help">Сохрани источник, чтобы обновить список доступных каналов.</p>
      )}

      {invalidConfig && (
        <p className="notice">
          Для звука на ПК выбери отдельные источник и выход, а также хотя бы один канал.
        </p>
      )}

      {error && (
        <p className="notice" role="alert">
          Не удалось сохранить настройки. Подробности доступны в диагностике.
        </p>
      )}

      <div className="panel-actions">
        <button onClick={onCancel}>Отмена</button>

        <button
          className="primary-button"
          disabled={busy.has("local") || invalidConfig || !loaded}
          onClick={() =>
            void action("local", async () => {
              await setLocalOutput(config);

              setDraft(null);
            })
          }
        >
          {busy.has("local") ? "Сохраняем…" : "Сохранить настройки"}
        </button>
      </div>

      <small className="help">
        {draft === null
          ? "Настройки сохранены. Можно закрыть это окно."
          : "Изменения применятся после сохранения."}
      </small>
    </>
  );
}
