//! Explicit hardware integration probe. Does not change Windows default devices.
//! Close audio players first; their audio would contaminate channel isolation.
use std::{
    thread,
    time::{Duration, Instant},
};
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

pub fn run(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (layout, rate) = crate::layout::read_endpoint(id)?;
    if let Some(error) = &layout.error {
        return Err(error.clone().into());
    }
    if rate != 48000 {
        return Err("Probe requires 48 kHz endpoint mix format".into());
    }
    let capture_id = crate::default_output::capture_endpoint(id, &layout)?;
    for selected in &layout.channels {
        let device = DeviceEnumerator::new()?.get_device(&capture_id)?;
        let mut client = device.get_iaudioclient()?;
        client.initialize_client(
            &WaveFormat::new(
                16,
                16,
                &SampleType::Int,
                48000,
                layout.channel_count,
                Some(layout.channel_mask),
            ),
            &Direction::Capture,
            &StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: 200_000,
            },
        )?;
        let event = client.set_get_eventhandle()?;
        let capture = client.get_audiocaptureclient()?;
        let mut bytes = vec![0; client.get_buffer_size()? as usize * layout.channel_count * 2];
        let mut energy = vec![0f64; layout.channel_count];
        let clock = crate::timing::Clock::new();
        let mut ages = Vec::new();
        let mut frames_total = 0usize;
        let mut first_signal_ms = None;
        client.start_stream()?;
        let target = id.to_owned();
        let speaker = selected.mask;
        let render =
            thread::spawn(move || crate::local_test::play_on_endpoint(Some(&target), speaker));
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(900) {
            let _ = event.wait_for_event(30);
            while capture.get_next_packet_size()?.unwrap_or(0) > 0 {
                let (frames, info) = capture.read_from_device(&mut bytes)?;
                frames_total += frames as usize;
                if !info.flags.timestamp_error && info.timestamp != 0 {
                    ages.push((clock.now() as i128 - info.timestamp as i128 * 100) as f64 / 1e6);
                }
                for frame in bytes[..frames as usize * layout.channel_count * 2]
                    .chunks_exact(layout.channel_count * 2)
                {
                    for (channel, sample) in frame.chunks_exact(2).enumerate() {
                        let value = i16::from_le_bytes([sample[0], sample[1]]) as f64;
                        if channel == selected.index
                            && value.abs() > 100.
                            && first_signal_ms.is_none()
                        {
                            first_signal_ms = Some(start.elapsed().as_secs_f64() * 1000.);
                        }
                        energy[channel] += value * value;
                    }
                }
            }
        }
        client.stop_stream()?;
        render.join().map_err(|_| "Renderer panicked")??;
        ages.sort_by(f64::total_cmp);
        let wanted = energy[selected.index];
        let unwanted = energy
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != selected.index)
            .map(|(_, v)| *v)
            .fold(0., f64::max);
        let isolated = wanted > 1_000_000. && unwanted < wanted * 0.001;
        println!(
            "{}",
            serde_json::json!({"type":"virtual_channel_probe","captureEndpoint":capture_id,"speaker":selected.code,"mask":layout.channel_mask,"channels":layout.channel_count,"frames":frames_total,"isolated":isolated,"energy":energy,"renderRequestToFirstSignalMs":first_signal_ms,"capturePacketAgeMedianMs":ages.get(ages.len()/2),"capturePacketAgeP95Ms":ages.get(ages.len().saturating_sub(1)*95/100)})
        );
        if !isolated {
            return Err(format!(
                "Channel {} is silent or leaks into other channels",
                selected.code
            )
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "hardware latency probe; sends impulses to an explicitly selected virtual endpoint"]
    fn steady_transfer_probe() -> Result<(), Box<dyn std::error::Error>> {
        use super::*;
        wasapi::initialize_mta().ok()?;
        let source = std::env::var("ROOMWAVE_PROBE_ENDPOINT")?;
        let target = std::env::var("ROOMWAVE_PROBE_CAPTURE_ENDPOINT").unwrap_or_else(|_|source.clone());
        let (layout,_) = crate::layout::read_endpoint(&source)?;
        let e=DeviceEnumerator::new()?;
        let f=WaveFormat::new(16,16,&SampleType::Int,48000,layout.channel_count,Some(layout.channel_mask));
        let mut r=e.get_device(&source)?.get_iaudioclient()?;
        let mut c=e.get_device(&target)?.get_iaudioclient()?;
        let mode=StreamMode::EventsShared{autoconvert:true,buffer_duration_hns:200_000};
        r.initialize_client(&f,&Direction::Render,&mode)?;
        c.initialize_client(&f,&Direction::Capture,&mode)?;
        let _re=r.set_get_eventhandle()?;
        let _ce=c.set_get_eventhandle()?;
        let render=r.get_audiorenderclient()?;
        let capture=c.get_audiocaptureclient()?;
        let align=layout.channel_count*2;
        let mut rb=vec![0;r.get_buffer_size()? as usize*align];
        let mut cb=vec![0;c.get_buffer_size()? as usize*align];
        let mut sent=std::collections::VecDeque::new();
        let mut delays=Vec::new();
        let mut frame=0usize;
        let mut last_high=false;
        c.start_stream()?;
        r.start_stream()?;
        let start=Instant::now();
        while start.elapsed()<Duration::from_secs(4) {
            let count=r.get_available_space_in_frames()? as usize;
            rb.fill(0);
            let mut markers=0;
            for n in 0..count {
                // One isolated impulse every 100 ms, after a 500 ms warmup.
                if frame+n>=24000 && (frame+n)%4800==0 {
                    rb[n*align..n*align+2].copy_from_slice(&12000i16.to_le_bytes());
                    markers+=1;
                }
            }
            if count>0 {
                let now=Instant::now();
                render.write_to_device(count,&rb[..count*align],None)?;
                for _ in 0..markers {sent.push_back(now);}
                frame+=count;
            }
            while capture.get_next_packet_size()?.unwrap_or(0)>0 {
                let (count,_)=capture.read_from_device(&mut cb)?;
                let now=Instant::now();
                for f in cb[..count as usize*align].chunks_exact(align) {
                    let high=i16::from_le_bytes([f[0],f[1]])>8000;
                    if high && !last_high {
                        if let Some(t)=sent.pop_front() {delays.push(now.duration_since(t).as_secs_f64()*1000.);}
                        else {return Err("Unexpected signal; close players before measuring".into());}
                    }
                    last_high=high;
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
        r.stop_stream()?;
        c.stop_stream()?;
        delays.sort_by(f64::total_cmp);
        println!("{}",serde_json::json!({"source":source,"capture":target,"scope":"render-enqueue-to-capture-read (includes render padding)","samples":delays.len(),"minMs":delays.first(),"medianMs":delays.get(delays.len()/2),"p95Ms":delays.get(delays.len().saturating_sub(1)*95/100)}));
        assert!(delays.len()>=25,"Insufficient returned impulses");
        Ok(())
    }
    #[test]
    #[ignore = "hardware diagnostic: plays short tones into the VB-CABLE endpoint only"]
    fn loopback_format_matrix() -> Result<(), Box<dyn std::error::Error>> {
        use super::*;
        wasapi::initialize_mta().ok()?;
        let id = std::env::var("ROOMWAVE_PROBE_ENDPOINT")?;
        let e = DeviceEnumerator::new()?;
        let (layout, _) = crate::layout::read_endpoint(&id)?;
        for float_capture in [false, true] {
            let capture_id =
                std::env::var("ROOMWAVE_PROBE_CAPTURE_ENDPOINT").unwrap_or_else(|_| id.clone());
            let d = e.get_device(&capture_id)?;
            let mut capture_client = d.get_iaudioclient()?;
            let cf = if float_capture {
                capture_client.get_mixformat()?
            } else {
                WaveFormat::new(
                    16,
                    16,
                    &SampleType::Int,
                    48000,
                    layout.channel_count,
                    Some(layout.channel_mask),
                )
            };
            capture_client.initialize_client(
                &cf,
                &Direction::Capture,
                &StreamMode::EventsShared {
                    autoconvert: !float_capture,
                    buffer_duration_hns: 200_000,
                },
            )?;
            let event = capture_client.set_get_eventhandle()?;
            let cap = capture_client.get_audiocaptureclient()?;
            let mut bytes =
                vec![0; capture_client.get_buffer_size()? as usize * cf.get_blockalign() as usize];
            capture_client.start_stream()?;
            for (channels, floating, convert) in [
                (2, true, true),
                (layout.channel_count, true, false),
                (2, false, true),
                (layout.channel_count, false, true),
            ] {
                let target = id.clone();
                let mask = if channels == 2 { 3 } else { layout.channel_mask };
                let render = thread::spawn(move || -> Result<(), String> {
                    let run = || -> Result<(), Box<dyn std::error::Error>> {
                        wasapi::initialize_mta().ok()?;
                        let mut a = DeviceEnumerator::new()?
                            .get_device(&target)?
                            .get_iaudioclient()?;
                        let bits = if floating { 32 } else { 16 };
                        let kind = if floating {
                            SampleType::Float
                        } else {
                            SampleType::Int
                        };
                        let f = WaveFormat::new(
                            bits,
                            bits,
                            &kind,
                            48000,
                            channels,
                            Some(mask),
                        );
                        a.initialize_client(
                            &f,
                            &Direction::Render,
                            &StreamMode::EventsShared {
                                autoconvert: convert,
                                buffer_duration_hns: 200_000,
                            },
                        )?;
                        let ev = a.set_get_eventhandle()?;
                        let r = a.get_audiorenderclient()?;
                        let capacity = a.get_buffer_size()? as usize;
                        let align = f.get_blockalign() as usize;
                        let mut buf = vec![0; capacity * align];
                        a.start_stream()?;
                        let t = Instant::now();
                        let mut n = 0u64;
                        while t.elapsed() < Duration::from_millis(700) {
                            ev.wait_for_event(500)?;
                            let count = a.get_available_space_in_frames()? as usize;
                            for frame in buf[..count * align].chunks_exact_mut(align) {
                                frame.fill(0);
                                let x =
                                    (n as f32 * 440. * std::f32::consts::TAU / 48000.).sin() * 0.1;
                                if floating {
                                    frame[..4].copy_from_slice(&x.to_le_bytes());
                                } else {
                                    frame[..2]
                                        .copy_from_slice(&((x * 32767.) as i16).to_le_bytes());
                                }
                                n += 1;
                            }
                            if count > 0 {
                                r.write_to_device(count, &buf[..count * align], None)?;
                            }
                        }
                        a.stop_stream()?;
                        Ok(())
                    };
                    run().map_err(|x| x.to_string())
                });
                let t = Instant::now();
                let mut peak = 0f32;
                while t.elapsed() < Duration::from_millis(1000) {
                    let _ = event.wait_for_event(30);
                    while cap.get_next_packet_size()?.unwrap_or(0) > 0 {
                        let (frames, _) = cap.read_from_device(&mut bytes)?;
                        let data = &bytes[..frames as usize * cf.get_blockalign() as usize];
                        for s in data.chunks_exact(if float_capture { 4 } else { 2 }) {
                            let x = if float_capture {
                                f32::from_le_bytes(s.try_into().unwrap())
                            } else {
                                i16::from_le_bytes(s.try_into().unwrap()) as f32 / 32768.
                            };
                            peak = peak.max(x.abs());
                        }
                    }
                }
                let rendered = render.join().map_err(|_| "Renderer panicked")?;
                println!("matrix captureMix={float_capture} renderChannels={channels} renderFloat={floating} convert={convert} peak={peak} renderResult={rendered:?}");
                rendered?;
                if std::env::var_os("ROOMWAVE_PROBE_REQUIRE_SIGNAL").is_some() {
                    assert!(peak > 0.01, "No audio in capture: {capture_id}");
                }
            }
            capture_client.stop_stream()?;
        }
        Ok(())
    }
    #[test]
    #[ignore = "plays all source channels; temporarily mutes other render sessions and restores them"]
    fn isolated_cable_probe() -> Result<(), Box<dyn std::error::Error>> {
        use windows::core::{Interface, HSTRING};
        use windows::Win32::{Media::Audio::*, System::Com::*};
        struct Restore(Vec<(ISimpleAudioVolume, bool)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (v, m) in &self.0 {
                    unsafe {
                        let _ = v.SetMute(*m, std::ptr::null());
                    }
                }
            }
        }
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let source = match std::env::var("ROOMWAVE_PROBE_ENDPOINT") {
                Ok(id) => id,
                Err(_) => wasapi::DeviceEnumerator::new()?
                    .get_default_device(&wasapi::Direction::Render)?
                    .get_id()?,
            };
            let d = e.GetDevice(&HSTRING::from(&source))?;
            let mgr: IAudioSessionManager2 = d.Activate(CLSCTX_ALL, None)?;
            let sessions = mgr.GetSessionEnumerator()?;
            let mut restore = Restore(Vec::new());
            for i in 0..sessions.GetCount()? {
                let s = sessions.GetSession(i)?;
                let c: IAudioSessionControl2 = s.cast()?;
                if c.GetProcessId()? == std::process::id() {
                    continue;
                }
                let v: ISimpleAudioVolume = s.cast()?;
                let mute = v.GetMute()?.as_bool();
                v.SetMute(true, std::ptr::null())?;
                restore.0.push((v, mute));
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
            super::run(&source)?;
        }
        Ok(())
    }
    #[test]
    #[ignore = "captures live system audio for format diagnostics"]
    fn passive_loopback_probe() -> Result<(), Box<dyn std::error::Error>> {
        use super::*;
        wasapi::initialize_mta().ok()?;
        let e = DeviceEnumerator::new()?;
        let source = e.get_default_device(&Direction::Render)?;
        let source_id = source.get_id()?;
        let (layout, _) = crate::layout::read_endpoint(&source_id)?;
        let record_id = crate::default_output::recording_endpoint(&source_id, &layout)?;
        for id in [&source_id, &record_id] {
            let d = e.get_device(id)?;
            let mut client = d.get_iaudioclient()?;
            let channels = layout.channel_count;
            client.initialize_client(
                &WaveFormat::new(
                    16,
                    16,
                    &SampleType::Int,
                    48000,
                    channels,
                    Some(layout.channel_mask),
                ),
                &Direction::Capture,
                &StreamMode::EventsShared {
                    autoconvert: true,
                    buffer_duration_hns: 200_000,
                },
            )?;
            let event = client.set_get_eventhandle()?;
            let cap = client.get_audiocaptureclient()?;
            let mut bytes = vec![0; client.get_buffer_size()? as usize * channels * 2];
            client.start_stream()?;
            let started = Instant::now();
            let mut peak = vec![0i32; channels];
            while started.elapsed() < Duration::from_secs(2) {
                let _ = event.wait_for_event(30);
                while cap.get_next_packet_size()?.unwrap_or(0) > 0 {
                    let (frames, _) = cap.read_from_device(&mut bytes)?;
                    for (i, s) in bytes[..frames as usize * channels * 2]
                        .chunks_exact(2)
                        .enumerate()
                    {
                        peak[i % channels] = peak[i % channels]
                            .max((i16::from_le_bytes(s.try_into().unwrap()) as i32).abs());
                    }
                }
            }
            client.stop_stream()?;
            println!("passive endpoint={id} channels={channels} peak={peak:?}");
        }
        Ok(())
    }
    #[test]
    #[ignore = "reads actual Windows audio endpoint/session volume"]
    fn cable_volume_probe() -> Result<(), Box<dyn std::error::Error>> {
        use windows::Win32::{
            Media::Audio::{Endpoints::*, *},
            System::Com::*,
        };
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            for direction in [eRender, eCapture] {
                let list = e.EnumAudioEndpoints(direction, DEVICE_STATE_ACTIVE)?;
                for i in 0..list.GetCount()? {
                    let d = list.Item(i)?;
                    let id = d.GetId()?;
                    let idstr = id.to_string()?;
                    let dev = wasapi::DeviceEnumerator::new()?.get_device(&idstr)?;
                    let name = dev.get_friendlyname()?;

                    let vol: IAudioEndpointVolume = d.Activate(CLSCTX_ALL, None)?;
                    println!(
                        "{name} {idstr} mute={:?} volume={} format={:?}",
                        vol.GetMute()?,
                        vol.GetMasterVolumeLevelScalar()?,
                        dev.get_iaudioclient()?.get_mixformat()?
                    );
                    let meter: IAudioMeterInformation = d.Activate(CLSCTX_ALL, None)?;
                    println!(
                        "ENDPOINT peak={} channel_volumes={:?}",
                        meter.GetPeakValue()?,
                        (0..vol.GetChannelCount()?)
                            .map(|i| vol.GetChannelVolumeLevelScalar(i).unwrap_or(-1.))
                            .collect::<Vec<_>>()
                    );
                    let mgr: IAudioSessionManager2 = d.Activate(CLSCTX_ALL, None)?;
                    let sessions = mgr.GetSessionEnumerator()?;
                    for n in 0..sessions.GetCount()? {
                        use windows::core::Interface;
                        let session = sessions.GetSession(n)?;
                        let v: ISimpleAudioVolume = session.cast()?;
                        let control: IAudioSessionControl2 = session.cast()?;
                        let meter: IAudioMeterInformation = session.cast()?;
                        println!(
                            "pid={} state={:?} peak={}",
                            control.GetProcessId()?,
                            session.GetState()?,
                            meter.GetPeakValue()?
                        );
                        println!(
                            "session mute={:?} vol={}",
                            v.GetMute()?,
                            v.GetMasterVolume()?
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
