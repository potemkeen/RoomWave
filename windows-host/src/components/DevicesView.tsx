import { connectDevice, disconnectAudio, refreshDiscovery, setChannel } from "../services/tauri";

import { Icon } from "./Icon";
import { speakerTitle } from "../lib/ui";

import type { Action, AudioState, DiscoverySnapshot, LocalConfig } from "../types/roomwave";

interface DevicesViewProps {
  audio: AudioState;
  snapshot: DiscoverySnapshot;

  loaded: boolean;
  busy: Set<string>;

  action: Action;

  openSettings: () => void;
  openDiagnostics: () => void;

  saveLocal: (config: LocalConfig) => void;
}

export function DevicesView({
  audio,
  snapshot,
  loaded,
  busy,
  action,
  openSettings,
  openDiagnostics,
  saveLocal,
}: DevicesViewProps) {
  const active = audio.receivers.filter((receiver) => receiver.status !== "disconnected");

  const devices = new Map(
    snapshot.devices.map((device) => [
      device.deviceId,
      {
        deviceId: device.deviceId,
        deviceName: device.deviceName,
        ipAddress: device.ipAddress,
      },
    ]),
  );

  for (const receiver of audio.receivers) {
    if (!devices.has(receiver.deviceId)) {
      devices.set(receiver.deviceId, {
        deviceId: receiver.deviceId,
        deviceName: receiver.deviceName,
        ipAddress: "—",
      });
    }
  }

  const available = snapshot.devices.filter(
    (device) => !active.some((receiver) => receiver.deviceId === device.deviceId),
  );

  const channels = audio.layout.channels;

  const outputName = audio.windowsOutputs.find(
    (device) => device.id === audio.localConfig.outputId,
  )?.name;

  const localProblem = audio.localConfig.enabled && Boolean(audio.localOutput.error);

  const canLocal = Boolean(
    audio.localConfig.sourceId &&
    audio.localConfig.outputId &&
    audio.localConfig.sourceId !== audio.localConfig.outputId &&
    audio.localConfig.speakers.length,
  );

  return (
    <>
      <section className="device-card">
        <div className="device-heading">
          <span className="device-icon">
            <Icon kind="pc" />
          </span>

          <div className="device-title">
            <h2>Этот компьютер</h2>

            <p>
              {audio.localConfig.enabled
                ? localProblem
                  ? "Не удалось включить звук"
                  : audio.localOutput.status === "synced"
                    ? "Готов к воспроизведению"
                    : "Подготавливаем звук…"
                : "Вывод через RoomWave выключен"}
            </p>
          </div>

          <button
            className={`toggle ${audio.localConfig.enabled ? "on" : ""}`}
            role="switch"
            aria-checked={audio.localConfig.enabled}
            aria-label="Звук на компьютере"
            disabled={!loaded || busy.has("local")}
            onClick={() => {
              if (!audio.localConfig.enabled && !canLocal) {
                openSettings();
                return;
              }

              saveLocal({
                ...audio.localConfig,
                enabled: !audio.localConfig.enabled,
              });
            }}
          >
            <span />
          </button>
        </div>

        <div className="device-body">
          <div className="route-label">КАНАЛЫ НА КОМПЬЮТЕРЕ</div>

          <div className="channel-chips">
            {channels.map((channel) => (
              <label
                className={audio.localConfig.speakers.includes(channel.mask) ? "chosen" : ""}
                key={channel.mask}
              >
                <input
                  type="checkbox"
                  checked={audio.localConfig.speakers.includes(channel.mask)}
                  disabled={
                    busy.has("local") ||
                    (audio.localConfig.enabled &&
                      audio.localConfig.speakers.length === 1 &&
                      audio.localConfig.speakers.includes(channel.mask))
                  }
                  onChange={(event) =>
                    saveLocal({
                      ...audio.localConfig,
                      speakers: event.target.checked
                        ? [...audio.localConfig.speakers, channel.mask]
                        : audio.localConfig.speakers.filter((speaker) => speaker !== channel.mask),
                    })
                  }
                />

                <span className="channel-code">{channel.code}</span>

                {speakerTitle(channel)}
              </label>
            ))}
          </div>

          {audio.localConfig.speakers.some(
            (speaker) => !channels.some((channel) => channel.mask === speaker),
          ) && (
            <p className="inline-warning">
              Некоторые каналы больше недоступны. Выбери замену в настройках.
            </p>
          )}

          <div className="card-foot">
            <span>{outputName ?? "Выбери наушники или колонки"}</span>

            <button className="text-button" onClick={openSettings}>
              Изменить
            </button>
          </div>

          {localProblem && (
            <div className="notice">
              Выход недоступен. Возможно, его использует другая программа.{" "}
              <button className="text-button" onClick={openDiagnostics}>
                Подробнее
              </button>
            </div>
          )}
        </div>
      </section>

      <div className="phones-heading">
        <h2>Телефоны</h2>

        <div className="button-row">
          {available.length > 1 && (
            <button
              className="text-button"
              disabled={busy.size > 0}
              onClick={() =>
                void action("all", async () => {
                  const results = await Promise.allSettled(
                    available.map((device) => connectDevice(device.deviceId)),
                  );

                  const failed = results.filter((result) => result.status === "rejected");

                  if (failed.length) {
                    throw Error(`Не удалось подключить устройств: ${failed.length}`);
                  }
                })
              }
            >
              Подключить все
            </button>
          )}

          <button
            className="text-button"
            disabled={busy.has("refresh")}
            onClick={() => void action("refresh", refreshDiscovery)}
          >
            {busy.has("refresh") ? "Ищем…" : "Обновить"}
          </button>
        </div>
      </div>

      {devices.size === 0 ? (
        <section className="empty-state">
          <span className="empty-icon">
            <Icon kind="phone" />
          </span>

          <h3>Телефоны не найдены</h3>

          <p>
            Открой RoomWave на телефоне и подключи его
            <br />к той же сети Wi-Fi. Он появится здесь автоматически.
          </p>

          <span className="searching">
            <span className="status-dot" />
            Ищем устройства поблизости
          </span>
        </section>
      ) : (
        <div className="device-list">
          {[...devices.values()].map((device) => {
            const receiver = audio.receivers.find((item) => item.deviceId === device.deviceId);

            const connected = receiver != null && receiver.status !== "disconnected";

            const disabled = busy.has(device.deviceId) || busy.has("all");

            const assigned = audio.assignments[device.deviceId];

            const channel = channels.find((item) => item.mask === assigned);

            const present = snapshot.devices.some((item) => item.deviceId === device.deviceId);

            return (
              <section className="device-card" key={device.deviceId}>
                <div className="device-heading">
                  <span className={`device-icon ${connected ? "connected" : ""}`}>
                    <Icon kind="phone" />
                  </span>

                  <div className="device-title">
                    <h2>{device.deviceName}</h2>

                    <p>
                      <span
                        className={`status-dot ${connected && !receiver?.error ? "" : "idle"}`}
                      />

                      {receiver?.error
                        ? "Подключение прервано"
                        : connected
                          ? receiver.status === "connecting"
                            ? "Подключаем…"
                            : receiver.syncStatus === "synced"
                              ? "Подключён"
                              : "Подготавливаем звук…"
                          : present
                            ? "Готов к подключению"
                            : "Не в сети"}
                    </p>
                  </div>

                  <button
                    className={connected ? "quiet-button" : "primary-button"}
                    disabled={disabled || (!connected && !present)}
                    onClick={() =>
                      void action(device.deviceId, () =>
                        connected
                          ? disconnectAudio(device.deviceId)
                          : connectDevice(device.deviceId),
                      )
                    }
                  >
                    {disabled ? "Подождите…" : connected ? "Отключить" : "Подключить"}
                  </button>
                </div>

                <div className="device-body phone-routing">
                  <label>
                    Что воспроизводить
                    <select
                      aria-label={`Канал для ${device.deviceName}`}
                      value={assigned ?? ""}
                      disabled={disabled}
                      onChange={(event) =>
                        void action(device.deviceId, () =>
                          setChannel(
                            device.deviceId,
                            event.target.value === "" ? null : Number(event.target.value),
                          ),
                        )
                      }
                    >
                      <option value="">Стерео · левый и правый</option>

                      {assigned !== undefined && !channel && (
                        <option value={assigned} disabled>
                          Канал недоступен — выбери другой
                        </option>
                      )}

                      {channels.map((item) => (
                        <option value={item.mask} key={item.mask}>
                          {speakerTitle(item)} · {item.code}
                        </option>
                      ))}
                    </select>
                  </label>

                  <small>
                    {assigned !== undefined && !channel
                      ? "Прежнего канала больше нет. Пока телефон будет молчать."
                      : "Звук воспроизводится через все динамики телефона."}
                  </small>

                  {receiver?.error && (
                    <button className="text-button" onClick={openDiagnostics}>
                      Посмотреть причину
                    </button>
                  )}
                </div>
              </section>
            );
          })}
        </div>
      )}

      {active.length > 1 && (
        <div className="end-actions">
          <button
            className="text-button"
            disabled={busy.size > 0}
            onClick={() => void action("all", () => disconnectAudio(null))}
          >
            Отключить все телефоны
          </button>
        </div>
      )}
    </>
  );
}
