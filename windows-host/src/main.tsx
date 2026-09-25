import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import "./style.css";

interface Device { deviceId: string; deviceName: string; ipAddress: string; port: number; protocolVersion: number }
interface DiscoverySnapshot { devices: Device[]; discovering: boolean; error: string | null }
interface ReceiverState {
  deviceId: string; deviceName: string; status: string; packetsSent: number;
  receiverPackets: number; receiverLost: number; latencyMs: number | null; rttMs: number | null;
  syncErrorMs: number | null; syncStatus: string; error: string | null;
  stages: Record<string, unknown> | null;
  transport?: Record<string, unknown>;
}
interface Speaker { mask: number; code: string; name: string; index: number }
interface LocalConfig { enabled: boolean; sourceId: string | null; outputId: string | null; speakers: number[] }
interface AudioState { streamMode?: "quality"|"latency"; modeRevision?: number; hostMetrics?: Record<string, unknown>; outputName: string | null; peak: number; targetDelayMs: number; receivers: ReceiverState[];
  layout: { name: string; channelCount: number; channelMask: number; channels: Speaker[]; error: string | null };
  assignments: Record<string, number>; captureError: string | null; testing: number | null;
  localConfig: LocalConfig; windowsOutputs: {id: string; name: string}[];
  localOutput: {status: string; error: string | null; syncErrorMs: number | null; latencyMs: number | null; outputMs: number | null; unavailable: number[]} }
const speakerPosition: Record<string, [number, number]> = {FL:[1,1],FR:[1,7],FC:[1,4],LFE:[5,4],BL:[5,1],BR:[5,7],SL:[3,1],SR:[3,7],FLC:[1,3],FRC:[1,5],BC:[5,3]};
const names: Record<string,string> = {FL:"Передний левый",FR:"Передний правый",FC:"Центр",LFE:"Сабвуфер",BL:"Задний левый",BR:"Задний правый",SL:"Боковой левый",SR:"Боковой правый",FLC:"Передний левый центральный",FRC:"Передний правый центральный",BC:"Задний центральный"};
const title = (c: Speaker) => names[c.code] ?? c.name;
const ms = (n: number | null | undefined) => n == null ? "—" : `${n.toFixed(1)} мс`;
// An explicit scalar allowlist keeps new backend diagnostics in logs, not in the UI.
function metric(source: unknown, ...path: string[]): number | null {
  let value = source;
  for (const key of path) {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    value = (value as Record<string, unknown>)[key];
  }
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}
const count = (n: number | null) => n == null ? "—" : n.toLocaleString("ru-RU");
const stateLabel = (state: string) => ({synced:"Синхронизировано",aligning:"Синхронизация",buffering:"Ожидание звука",connecting:"Подключение",streaming:"Подключено",error:"Ошибка",idle:"Не активно",preparing:"Подготовка",warming:"Подготовка",audioClock:"Подготовка аудиовывода",disconnected:"Отключено",waiting:"Ожидание"}[state] ?? "—");
function MetricRows({rows}:{rows: [string,string][]}) {
  return <dl>{rows.map(([label,value])=><React.Fragment key={label}><dt>{label}</dt><dd>{value}</dd></React.Fragment>)}</dl>;
}
function Icon({kind}:{kind:"wave"|"pc"|"phone"|"settings"|"chart"|"sound"}) {
  const paths = {wave:"M3 10v4m4-8v12m5-16v20m5-16v12m4-8v4",pc:"M3 4h18v13H3zM8 21h8m-4-4v4",phone:"M7 2h10v20H7zM11 18h2",settings:"M4 7h16M4 17h16M8 4v6m8 4v6",chart:"M4 20V10m8 10V4m8 16v-7",sound:"M4 9h4l5-4v14l-5-4H4zM17 8q5 4 0 8"};
  return <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[kind]}/></svg>;
}
function Panel({title:heading,onClose,children}:{title:string;onClose:()=>void;children:React.ReactNode}) {
  const ref=React.useRef<HTMLDialogElement>(null);
  React.useEffect(()=>{const dialog=ref.current!;dialog.showModal();return()=>dialog.close();},[]);
  return <dialog ref={ref} onCancel={onClose} aria-labelledby="panel-title" onClick={e=>{if(e.target===e.currentTarget)onClose();}}>
    <div className="panel-inner"><div className="panel-heading"><h2 id="panel-title">{heading}</h2><button className="icon-button" onClick={onClose} aria-label="Закрыть">✕</button></div>{children}</div>
  </dialog>;
}
interface VirtualAudio { endpointId: string|null; previousOutputId: string|null; active: boolean; error: string|null }
function App() {
  const [virtualAudio,setVirtualAudio]=React.useState<VirtualAudio|null>(null);
  const [logState,setLogState]=React.useState<{path:string|null;recording:boolean;samples:number;error:string|null;endsAtUnixMs:number|null}|null>(null);
  const [snapshot,setSnapshot]=React.useState<DiscoverySnapshot>({devices:[],discovering:false,error:null});
  const [audio,setAudio]=React.useState<AudioState>({outputName:null,peak:0,targetDelayMs:80,receivers:[],layout:{name:"…",channelCount:0,channelMask:0,channels:[],error:null},assignments:{},captureError:null,testing:null,localConfig:{enabled:false,sourceId:null,outputId:null,speakers:[]},windowsOutputs:[],localOutput:{status:"disabled",error:null,syncErrorMs:null,latencyMs:null,outputMs:null,unavailable:[]}});
  const [loaded,setLoaded]=React.useState(false);
  const [panel,setPanel]=React.useState<"settings"|"diagnostics"|null>(null);
  const [view,setView]=React.useState<"devices"|"test">("devices");
  const [draft,setDraft]=React.useState<LocalConfig|null>(null);
  const [error,setError]=React.useState<string|null>(null);
  const [flashing,setFlashing]=React.useState<number|null>(null);
  const [busy,setBusy]=React.useState<Set<string>>(new Set());
  const pending=React.useRef(new Set<string>());
  const config=draft??audio.localConfig;
  React.useEffect(()=>{
    let disposed=false;let timer:ReturnType<typeof setTimeout>;
    async function update(){try{const [devices,state,log,virtualState]=await Promise.all([invoke<DiscoverySnapshot>("get_discovery_state"),invoke<AudioState>("get_audio_state"),invoke<{path:string|null;recording:boolean;samples:number;error:string|null;endsAtUnixMs:number|null}>("get_diagnostic_log_state"),invoke<VirtualAudio>("get_virtual_audio_state")]);if(!disposed){setSnapshot(devices);setAudio(state);setLogState(log);setVirtualAudio(virtualState);setLoaded(true);}}
      catch(e){if(!disposed)setError(String(e));}finally{if(!disposed)timer=setTimeout(update,500);}}
    void update();return()=>{disposed=true;clearTimeout(timer);};
  },[]);
  async function action(key:string,run:()=>Promise<unknown>){
    if(pending.current.has(key))return;
    pending.current.add(key);setBusy(new Set(pending.current));
    try{await run();setAudio(await invoke<AudioState>("get_audio_state"));setError(null);}
    catch(e){setError(String(e));}finally{pending.current.delete(key);setBusy(new Set(pending.current));}
  }
  const active=audio.receivers.filter(r=>r.status!=="disconnected");
  const devices=new Map(snapshot.devices.map(d=>[d.deviceId,{deviceId:d.deviceId,deviceName:d.deviceName,ipAddress:d.ipAddress}]));
  for(const r of audio.receivers)if(!devices.has(r.deviceId))devices.set(r.deviceId,{deviceId:r.deviceId,deviceName:r.deviceName,ipAddress:"—"});
  const available=snapshot.devices.filter(d=>!active.some(r=>r.deviceId===d.deviceId));
  const channels=audio.layout.channels;
  const outputName=audio.windowsOutputs.find(d=>d.id===audio.localConfig.outputId)?.name;
  const problem=Boolean(error||snapshot.error||audio.captureError||audio.layout.error||virtualAudio?.error);
  const localProblem=audio.localConfig.enabled&&Boolean(audio.localOutput.error);
  function openSettings(){
    const next={...audio.localConfig,speakers:[...audio.localConfig.speakers]};
    // Suggest only unambiguous devices; never overwrite a saved or missing endpoint.
    if(!next.sourceId){const matching=audio.windowsOutputs.filter(d=>d.name===audio.outputName);if(matching.length===1)next.sourceId=matching[0].id;}
    if(!next.outputId){const matching=audio.windowsOutputs.filter(d=>d.id!==next.sourceId&&!/voicemeeter|virtual|cable|digital|streaming|nvidia|amd|hdmi/i.test(d.name));if(matching.length===1)next.outputId=matching[0].id;}
    if(!next.speakers.length)next.speakers=channels.filter(c=>[1,2,4].includes(c.mask)).map(c=>c.mask);
    setDraft(next);setPanel("settings");
  }
  const canLocal=Boolean(audio.localConfig.sourceId&&audio.localConfig.outputId&&audio.localConfig.sourceId!==audio.localConfig.outputId&&audio.localConfig.speakers.length);
  const invalidConfig=config.enabled&&(!config.sourceId||!config.outputId||config.sourceId===config.outputId||config.speakers.length===0);
  const saveLocal=(next:LocalConfig)=>void action("local",()=>invoke("set_local_output",{config:next}));
  return <main>
    <header className="app-header"><div className="brand"><span className="brand-icon"><Icon kind="wave"/></span><span>RoomWave</span></div>
      <nav aria-label="Меню"><button className="icon-button" title="Диагностика" aria-label="Открыть диагностику" onClick={()=>setPanel("diagnostics")}><Icon kind="chart"/></button><button className="icon-button" title="Настройки звука" aria-label="Открыть настройки звука" onClick={openSettings}><Icon kind="settings"/></button></nav>
    </header>
    <div className="source-strip"><span className={`status-dot ${problem?'warn':''}`}/><span>{!loaded?"Подготавливаем звук…":problem?"Нужно проверить источник звука":"Источник звука готов"}<small>{loaded?(audio.layout.name==="Stereo"?"Стерео":audio.layout.name==="Mono"?"Моно":audio.layout.name):"Определяем доступные каналы"}</small></span><button className="text-button" onClick={openSettings}>Настроить</button></div>

    {problem&&<div className="notice" role="alert">Не удалось подготовить звук или обновить устройства. Проверь источник в настройках. <button className="text-button" onClick={()=>setPanel("diagnostics")}>Подробнее</button></div>}
    <div className="section-toolbar"><div className="tabs" role="group" aria-label="Раздел"><button aria-pressed={view==="devices"} className={view==="devices"?'selected':''} onClick={()=>setView("devices")}>Устройства</button><button aria-pressed={view==="test"} className={view==="test"?'selected':''} onClick={()=>setView("test")}>Проверка звука</button></div>{view==="devices"&&<span className="count">{active.length} подключено</span>}</div>
    {view==="devices"?<>
      <section className="device-card"><div className="device-heading"><span className="device-icon"><Icon kind="pc"/></span><div className="device-title"><h2>Этот компьютер</h2><p>{audio.localConfig.enabled?(localProblem?"Не удалось включить звук":audio.localOutput.status==="synced"?"Готов к воспроизведению":"Подготавливаем звук…"):"Вывод через RoomWave выключен"}</p></div><button className={`toggle ${audio.localConfig.enabled?'on':''}`} role="switch" aria-checked={audio.localConfig.enabled} aria-label="Звук на компьютере" disabled={!loaded||busy.has("local")} onClick={()=>{if(!audio.localConfig.enabled&&!canLocal){openSettings();return;}saveLocal({...audio.localConfig,enabled:!audio.localConfig.enabled});}}><span/></button></div>
        <div className="device-body"><div className="route-label">КАНАЛЫ НА КОМПЬЮТЕРЕ</div><div className="channel-chips">{channels.map(c=><label className={audio.localConfig.speakers.includes(c.mask)?'chosen':''} key={c.mask}><input type="checkbox" checked={audio.localConfig.speakers.includes(c.mask)} disabled={busy.has("local")||(audio.localConfig.enabled&&audio.localConfig.speakers.length===1&&audio.localConfig.speakers.includes(c.mask))} onChange={e=>saveLocal({...audio.localConfig,speakers:e.target.checked?[...audio.localConfig.speakers,c.mask]:audio.localConfig.speakers.filter(s=>s!==c.mask)})}/><span className="channel-code">{c.code}</span>{title(c)}</label>)}</div>
        {audio.localConfig.speakers.some(s=>!channels.some(c=>c.mask===s))&&<p className="inline-warning">Некоторые каналы больше недоступны. Выбери замену в настройках.</p>}
        <div className="card-foot"><span>{outputName??"Выбери наушники или колонки"}</span><button className="text-button" onClick={openSettings}>Изменить</button></div>
        {localProblem&&<div className="notice">Выход недоступен. Возможно, его использует другая программа. <button className="text-button" onClick={()=>setPanel("diagnostics")}>Подробнее</button></div>}</div>
      </section>
      <div className="phones-heading"><h2>Телефоны</h2><div className="button-row">{available.length>1&&<button className="text-button" disabled={busy.size>0} onClick={()=>void action("all",async()=>{const results=await Promise.allSettled(available.map(d=>invoke("connect_device",{deviceId:d.deviceId})));const failed=results.filter(r=>r.status==='rejected');if(failed.length)throw Error(`Не удалось подключить устройств: ${failed.length}`);})}>Подключить все</button>}<button className="text-button" disabled={busy.has("refresh")} onClick={()=>void action("refresh",()=>invoke("refresh_discovery"))}>{busy.has("refresh")?"Ищем…":"Обновить"}</button></div></div>
      {devices.size===0?<section className="empty-state"><span className="empty-icon"><Icon kind="phone"/></span><h3>Телефоны не найдены</h3><p>Открой RoomWave на телефоне и подключи его<br/>к той же сети Wi-Fi. Он появится здесь автоматически.</p><span className="searching"><span className="status-dot"/>Ищем устройства поблизости</span></section>:<div className="device-list">{[...devices.values()].map(device=>{
        const receiver=audio.receivers.find(r=>r.deviceId===device.deviceId);const connected=receiver!=null&&receiver.status!=="disconnected";const disabled=busy.has(device.deviceId)||busy.has("all");const assigned=audio.assignments[device.deviceId];const channel=channels.find(c=>c.mask===assigned);const present=snapshot.devices.some(d=>d.deviceId===device.deviceId);
        return <section className="device-card" key={device.deviceId}><div className="device-heading"><span className={`device-icon ${connected?'connected':''}`}><Icon kind="phone"/></span><div className="device-title"><h2>{device.deviceName}</h2><p><span className={`status-dot ${connected&&!receiver?.error?'':'idle'}`}/>{receiver?.error?"Подключение прервано":connected?(receiver.status==="connecting"?"Подключаем…":receiver.syncStatus==="synced"?"Подключён":"Подготавливаем звук…"):present?"Готов к подключению":"Не в сети"}</p></div><button className={connected?'quiet-button':'primary-button'} disabled={disabled||(!connected&&!present)} onClick={()=>void action(device.deviceId,()=>connected?invoke("disconnect_audio",{deviceId:device.deviceId}):invoke("connect_device",{deviceId:device.deviceId}))}>{disabled?"Подождите…":connected?"Отключить":"Подключить"}</button></div>
          <div className="device-body phone-routing"><label>Что воспроизводить<select aria-label={`Канал для ${device.deviceName}`} value={assigned??""} disabled={disabled} onChange={e=>void action(device.deviceId,()=>invoke("set_channel",{deviceId:device.deviceId,speaker:e.target.value===""?null:Number(e.target.value)}))}><option value="">Стерео · левый и правый</option>{assigned!==undefined&&!channel&&<option value={assigned} disabled>Канал недоступен — выбери другой</option>}{channels.map(c=><option value={c.mask} key={c.mask}>{title(c)} · {c.code}</option>)}</select></label><small>{assigned!==undefined&&!channel?"Прежнего канала больше нет. Пока телефон будет молчать.":"Звук воспроизводится через все динамики телефона."}</small>{receiver?.error&&<button className="text-button" onClick={()=>setPanel("diagnostics")}>Посмотреть причину</button>}</div>
        </section>;
      })}</div>}
      {active.length>1&&<div className="end-actions"><button className="text-button" disabled={busy.size>0} onClick={()=>void action("all",()=>invoke("disconnect_audio",{deviceId:null}))}>Отключить все телефоны</button></div>}
    </>:<section className="test-card"><div className="test-heading"><span className="device-icon"><Icon kind="sound"/></span><div><h2>Проверка каналов</h2><p>Нажми на динамик, чтобы услышать короткий сигнал.</p></div></div><div className="speaker-map" aria-label={`Схема ${audio.layout.name}`}><span className="listener"><span>◎</span>Слушатель</span>{channels.map((c,i)=>{const [row,column]=speakerPosition[c.code]??[7,i+1];return <button key={c.mask} aria-label={`Проверить: ${title(c)}`} className={`speaker ${audio.testing===c.mask||flashing===c.mask?'playing':''}`} style={{gridRow:row,gridColumn:column}} disabled={busy.has("test")||Boolean(audio.captureError||audio.layout.error)} onClick={()=>void action("test",async()=>{setFlashing(c.mask);try{await invoke("test_speaker",{speaker:c.mask});}finally{setFlashing(null);}})}><Icon kind="sound"/><strong>{c.code}</strong><small>{title(c)}</small></button>;})}</div><p className="test-hint">Сигнал идёт на устройства, которым назначен канал. Если вывод ПК выключен, тест также использует выход Windows по умолчанию.</p></section>}
    {panel&&<Panel title={panel==="settings"?"Настройки звука":"Диагностика"} onClose={()=>{setPanel(null);setDraft(null);}}>{panel==="settings"?<>
      {virtualAudio?.error&&<p className="error">{virtualAudio.error}</p>}<p className="panel-description">{virtualAudio?.endpointId ? "Источник RoomWave настроен автоматически. Выбери выход ПК и назначь каналы." : "Выбери источник звука и выход ПК. Для автоматической настройки установи драйвер RoomWave Virtual Speakers."}</p>
      <label className="field">Источник звука<select value={config.sourceId??""} disabled={busy.has("local")||Boolean(virtualAudio?.endpointId)} onChange={e=>setDraft({...config,sourceId:e.target.value||null})}><option value="">Как в Windows</option>{config.sourceId&&!audio.windowsOutputs.some(d=>d.id===config.sourceId)&&<option value={config.sourceId}>Сохранённый источник недоступен</option>}{audio.windowsOutputs.map(d=><option key={d.id} value={d.id}>{d.name}</option>)}</select></label>
      <p className="help">{virtualAudio?.endpointId ? "Windows переключается на RoomWave при запуске. После закрытия прежний выход восстанавливается, если ты не переключил его вручную." : "Для объёмного звука нужен источник с каналами 5.1/7.1. В Windows или плеере должен быть выбран тот же выход."}</p>
      <label className="field">Наушники или колонки ПК<select value={config.outputId??""} disabled={busy.has("local")} onChange={e=>setDraft({...config,outputId:e.target.value||null})}><option value="">Выбери устройство</option>{config.outputId&&!audio.windowsOutputs.some(d=>d.id===config.outputId)&&<option value={config.outputId}>Сохранённый выход недоступен</option>}{audio.windowsOutputs.filter(d=>d.id!==virtualAudio?.endpointId&&(d.id!==config.sourceId||d.id===config.outputId)).map(d=><option key={d.id} value={d.id}>{d.name}</option>)}</select></label>
      <p className="help">Для наушников оставь схему Stereo в Windows: центральный канал будет слышен с обеих сторон.</p>
      <label className="check-row"><input type="checkbox" checked={config.enabled} disabled={busy.has("local")} onChange={e=>setDraft({...config,enabled:e.target.checked})}/>Воспроизводить выбранные каналы на ПК</label>
      <fieldset><legend>Каналы компьютера</legend><div className="channel-chips">{channels.map(c=><label key={c.mask} className={config.speakers.includes(c.mask)?'chosen':''}><input type="checkbox" disabled={busy.has("local")} checked={config.speakers.includes(c.mask)} onChange={e=>setDraft({...config,speakers:e.target.checked?[...config.speakers,c.mask]:config.speakers.filter(s=>s!==c.mask)})}/>{title(c)}</label>)}{config.speakers.filter(s=>!channels.some(c=>c.mask===s)).map(s=><label key={s}><input type="checkbox" checked disabled={busy.has("local")} onChange={()=>setDraft({...config,speakers:config.speakers.filter(v=>v!==s)})}/>Недоступный канал ({s})</label>)}</div></fieldset>
      {config.sourceId!==audio.localConfig.sourceId&&<p className="help">Сохрани источник, чтобы обновить список доступных каналов.</p>}
      {invalidConfig&&<p className="notice">Для звука на ПК выбери отдельные источник и выход, а также хотя бы один канал.</p>}
      {error&&<p className="notice" role="alert">Не удалось сохранить настройки. Подробности доступны в диагностике.</p>}
      <div className="panel-actions"><button onClick={()=>{setPanel(null);setDraft(null);}}>Отмена</button><button className="primary-button" disabled={busy.has("local")||invalidConfig||!loaded} onClick={()=>void action("local",async()=>{await invoke("set_local_output",{config});setDraft(null);})}>{busy.has("local")?"Сохраняем…":"Сохранить настройки"}</button></div><small className="help">{draft===null?"Настройки сохранены. Можно закрыть это окно.":"Изменения применятся после сохранения."}</small>
    </>:<>
      <section className="diagnostic-block"><h3>Аудиодрайвер</h3><p className="help">VB-CABLE от VB-Audio — donationware. Если драйвер полезен, автор приветствует оплату лицензии или пожертвование. Сайт: https://vb-cable.com · Условия: https://vb-audio.com/Services/licensing.htm</p></section>
      <section className="diagnostic-block"><h3>Запись сеанса</h3><p className="help">{logState?.recording ? `Записывается · ${logState.samples} замеров · осталось ${Math.max(0,Math.ceil(((logState.endsAtUnixMs??Date.now())-Date.now())/60000))} мин` : "Подробная запись выключена"}</p><p className="help">Для поиска проблем можно записать показатели на 10 минут. Запись остановится автоматически. Звук не сохраняется.</p><button disabled={busy.has("logging")} onClick={()=>void action("logging",async()=>{await invoke(logState?.recording?"stop_diagnostic_log":"start_diagnostic_log");setLogState(await invoke("get_diagnostic_log_state"));})}>{logState?.recording?"Остановить запись":"Записать сеанс · 10 минут"}</button>{logState?.path&&<><button disabled={busy.has("logging")} onClick={()=>void action("logging",()=>invoke("reveal_diagnostic_log"))}>Показать файл</button><pre className="log-path">{logState.path}</pre></>}{logState?.error&&<pre className="error-detail">{logState.error}</pre>}{error&&<p className="error-detail">{error}</p>}<p className="help">Отдельно сохраняется небольшой журнал ошибок: errors.log в папке логов.</p></section>
      <p className="panel-description">Задержка измеряется от получения PCM хостом до расчётного воспроизведения. Буфер виртуального кабеля до захвата и акустическая задержка динамиков в неё не входят; полная задержка пока не измерена.</p>
      <section className="diagnostic-block"><h3>Источник</h3><MetricRows rows={[
        ["Устройство",audio.outputName??"—"],
        ["Схема звука",audio.layout.name],
        ["Уровень сигнала",`${Math.round(audio.peak*100)}%`],
        ["Запас перед воспроизведением",ms(audio.targetDelayMs)],
      ]}/></section>
      {audio.localConfig.enabled&&<section className="diagnostic-block"><h3>Звук на ПК</h3><MetricRows rows={[
        ["Устройство",outputName??"—"],
        ["Состояние",stateLabel(audio.localOutput.status)],
        ["Задержка",ms(audio.localOutput.latencyMs)],
        ["Отклонение синхронизации",ms(audio.localOutput.syncErrorMs)],
        ["Буфер вывода",ms(audio.localOutput.outputMs)],
      ]}/></section>}
      {[error,snapshot.error,audio.captureError,audio.layout.error,audio.localOutput.error,virtualAudio?.error].filter(Boolean).map((e,i)=><pre className="error-detail" key={i}>{e}</pre>)}
      {audio.hostMetrics&&<section className="diagnostic-block"><h3>Обработка на ПК</h3><MetricRows rows={[
        ["Период захвата Windows",ms(metric(audio.hostMetrics,"captureStages","engine","currentPeriodMs"))],
        ["Чтение → передача, p95",ms(metric(audio.hostMetrics,"captureStages","readToPublish","p95Ms"))],
        ["Пропуски в очередях",count(metric(audio.hostMetrics,"subscriberQueueDrops"))],
      ]}/><p className="help">p95 — время, в которое укладываются 95% блоков.</p></section>}
      {audio.receivers.map(r=><section className="diagnostic-block" key={r.deviceId}><h3>{r.deviceName}</h3><MetricRows rows={[
        ["Соединение",r.error?"Ошибка":stateLabel(r.status)],
        ["Синхронизация",stateLabel(r.syncStatus)],
        ["Задержка",ms(r.latencyMs)],
        ["Сеть (RTT)",ms(r.rttMs)],
        ["Колебания задержки сети",ms(metric(r.stages,"jitterMs"))],
        ["Отклонение синхронизации",ms(r.syncErrorMs)],
        ["Буфер перед воспроизведением",ms(metric(r.stages,"jitterBufferMs"))],
        ["Буфер аудиовывода",ms(metric(r.stages,"audioOutputMs"))],
        ["Пропущено пакетов",count(r.receiverLost)],
        ["Опоздало пакетов",count(metric(r.stages,"latePackets"))],
        ["Нехватка данных для вывода",count(metric(r.stages,"underruns"))],
      ]}/><p className="help">Счётчики — с начала подключения. Отсутствующие данные обозначены «—».</p>{r.error&&<pre className="error-detail">{r.error}</pre>}</section>)}
      {audio.receivers.length===0&&<p className="help">Метрики телефонов появятся после подключения.</p>}
    </>}</Panel>}
  </main>;
}
ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode><App/></React.StrictMode>);