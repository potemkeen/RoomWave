import React from "react";
import { testSpeaker } from "../services/tauri";

import { Icon } from "./Icon";
import { speakerPosition, speakerTitle } from "../lib/ui";

import type { Action, AudioState } from "../types/roomwave";

interface SpeakerTestProps {
  audio: AudioState;
  busy: Set<string>;

  flashing: number | null;
  setFlashing: React.Dispatch<React.SetStateAction<number | null>>;

  action: Action;
}

export function SpeakerTest({ audio, busy, flashing, setFlashing, action }: SpeakerTestProps) {
  const channels = audio.layout.channels;

  return (
    <section className="test-card">
      <div className="test-heading">
        <span className="device-icon">
          <Icon kind="sound" />
        </span>

        <div>
          <h2>Проверка каналов</h2>

          <p>Нажми на динамик, чтобы услышать короткий сигнал.</p>
        </div>
      </div>

      <div className="speaker-map" aria-label={`Схема ${audio.layout.name}`}>
        <span className="listener">
          <span>◎</span>
          Слушатель
        </span>

        {channels.map((channel, index) => {
          const [row, column] = speakerPosition[channel.code] ?? [7, index + 1];

          return (
            <button
              key={channel.mask}
              aria-label={`Проверить: ${speakerTitle(channel)}`}
              className={`speaker ${
                audio.testing === channel.mask || flashing === channel.mask ? "playing" : ""
              }`}
              style={{
                gridRow: row,
                gridColumn: column,
              }}
              disabled={busy.has("test") || Boolean(audio.captureError || audio.layout.error)}
              onClick={() =>
                void action("test", async () => {
                  setFlashing(channel.mask);

                  try {
                    await testSpeaker(channel.mask);
                  } finally {
                    setFlashing(null);
                  }
                })
              }
            >
              <Icon kind="sound" />

              <strong>{channel.code}</strong>

              <small>{speakerTitle(channel)}</small>
            </button>
          );
        })}
      </div>

      <p className="test-hint">
        Сигнал идёт на устройства, которым назначен канал. Если вывод ПК выключен, тест также
        использует выход Windows по умолчанию.
      </p>
    </section>
  );
}
