//! The guardian owns default-device changes, never the audio hot path.
//! Its stdin closes when the host exits or crashes. A durable journal also permits
//! recovery after the guardian or the machine itself dies. No service/admin needed.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{mpsc, Mutex},
    thread,
    time::Duration,
};
use windows::{
    core::{implement, IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT, PCWSTR, PWSTR},
    Win32::{
        Foundation::PROPERTYKEY,
        Media::Audio::*,
        System::Com::{
            StructuredStorage::{PropVariantClear, PropVariantToStringAlloc},
            *,
        },
    },
};
type Res<T> = Result<T, Box<dyn std::error::Error>>;
const MARKER: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x44d940c1_b284_46aa_b940_7db95e2f67a9),
    pid: 1,
};
const MARKER_VALUE: &str = "RoomWave.VirtualSpeakers.v1";
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub endpoint_id: Option<String>,
    pub previous_output_id: Option<String>,
    pub active: bool,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct RoleLease {
    role: i32,
    previous: String,
    owned: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct Journal {
    version: u32,
    target: String,
    roles: Vec<RoleLease>,
}
impl Journal {
    fn observe(&mut self, role: i32, current: Option<&str>) {
        if current != Some(self.target.as_str()) {
            for lease in &mut self.roles {
                if lease.role == role {
                    lease.owned = false;
                }
            }
        }
    }
    fn restore_to(&self, role: i32, current: Option<&str>) -> Option<&str> {
        self.roles
            .iter()
            .find(|r| {
                r.role == role
                    && r.owned
                    && r.previous != self.target
                    && current == Some(self.target.as_str())
            })
            .map(|r| r.previous.as_str())
    }
}
fn directory() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("RoomWave")
}
fn journal_path() -> PathBuf {
    directory().join("default-output-recovery.json")
}
fn save(j: &Journal) -> Res<()> {
    let path = journal_path();
    let temp = path.with_extension("tmp");
    let mut f = fs::File::create(&temp)?;
    f.write_all(&serde_json::to_vec(j)?)?;
    f.sync_all()?;
    drop(f);
    // Windows rename replaces an existing file atomically on the same volume.
    fs::rename(temp, path)?;
    Ok(())
}
unsafe fn take_string(p: PWSTR) -> String {
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0.cast())) };
    s
}
fn device_id(d: &IMMDevice) -> Res<String> {
    Ok(unsafe { take_string(d.GetId()?) })
}
fn current(e: &IMMDeviceEnumerator, role: i32) -> Option<String> {
    unsafe { e.GetDefaultAudioEndpoint(eRender, ERole(role)) }
        .ok()
        .and_then(|d| device_id(&d).ok())
}
fn present(e: &IMMDeviceEnumerator, id: &str) -> bool {
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    unsafe {
        e.GetDevice(PCWSTR(wide.as_ptr()))
            .and_then(|d| d.GetState())
    }
    .is_ok_and(|s| s == DEVICE_STATE_ACTIVE)
}
fn find_virtual(e: &IMMDeviceEnumerator) -> Res<Option<String>> {
    let list = unsafe { e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)? };
    let mut own = None;
    for i in 0..unsafe { list.GetCount()? } {
        let d = unsafe { list.Item(i)? };
        let store = unsafe { d.OpenPropertyStore(STGM_READ)? };
        // Signed VB-CABLE speaker endpoint; independent of localized display names.
        let driver = PROPERTYKEY {
            fmtid: GUID::from_u128(0xa8b865dd_2e3d_4094_ad97_e593a70c75d6),
            pid: 8,
        };
        let form = PROPERTYKEY {
            fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e),
            pid: 0,
        };
        let read = |key: &PROPERTYKEY| -> Option<String> {
            let mut v = unsafe { store.GetValue(key) }.ok()?;
            let result = unsafe { PropVariantToStringAlloc(&v) }
                .ok()
                .map(|p| unsafe { take_string(p) });
            unsafe {
                let _ = PropVariantClear(&mut v);
            }
            result
        };
        if read(&driver).as_deref() == Some("VBAudioVACWDM") && read(&form).as_deref() == Some("1")
        {
            return Ok(Some(device_id(&d)?));
        }
        if let Ok(mut value) = unsafe { store.GetValue(&MARKER) } {
            let text = unsafe { PropVariantToStringAlloc(&value) }
                .ok()
                .map(|p| unsafe { take_string(p) });
            unsafe {
                let _ = PropVariantClear(&mut value);
            }
            if text.as_deref() == Some(MARKER_VALUE) {
                own = Some(device_id(&d)?);
            }
        }
    }
    Ok(own)
}
// Windows has no documented desktop SetDefaultAudioEndpoint API. Keep the
// IPolicyConfig ABI isolated here; fail without changing anything if unavailable.
// IID/layout verified against AudioDeviceCmdlets/SOURCE/PolicyConfigClient.cs.
#[repr(C)]
struct PolicyVtable {
    base: IUnknown_Vtbl,
    format_queries: [usize; 3],
    set_device_format: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        PCWSTR,
        *const WAVEFORMATEX,
        *const WAVEFORMATEX,
    ) -> HRESULT,
    unused: [usize; 6],
    set_default: unsafe extern "system" fn(*mut std::ffi::c_void, PCWSTR, ERole) -> HRESULT,
}
fn policy_client() -> Res<IUnknown> {
    let object: IUnknown = unsafe {
        CoCreateInstance(
            &GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9),
            None,
            CLSCTX_ALL,
        )?
    };
    let mut raw = std::ptr::null_mut();
    unsafe {
        object
            .query(
                &GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8),
                &mut raw,
            )
            .ok()?;
    }
    Ok(unsafe { IUnknown::from_raw(raw) })
}
fn set_default(id: &str, role: i32) -> Res<()> {
    let policy = policy_client()?;
    let vtable = unsafe { *(policy.as_raw() as *const *const PolicyVtable) };
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    unsafe {
        ((*vtable).set_default)(policy.as_raw(), PCWSTR(wide.as_ptr()), ERole(role)).ok()?;
    }
    Ok(())
}

/// Capture the render mixer directly. VB-CABLE requires its vendor-panel
/// "Enable Loopback Streaming" option; the recording pin adds a cable queue.
pub fn capture_endpoint(source: &str, _layout: &crate::layout::Layout) -> Res<String> {
    Ok(source.to_owned())
}

/// Explicit diagnostic comparison only; never silently add cable latency to playback.
#[cfg(test)]
pub fn recording_endpoint(source: &str, layout: &crate::layout::Layout) -> Res<String> {
    let e: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let driver_key = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa8b865dd_2e3d_4094_ad97_e593a70c75d6),
        pid: 8,
    };
    let driver = |d: &IMMDevice| -> Res<String> {
        let store = unsafe { d.OpenPropertyStore(STGM_READ)? };
        let mut value = unsafe { store.GetValue(&driver_key)? };
        let result = unsafe { PropVariantToStringAlloc(&value) }.map(|p| unsafe { take_string(p) });
        unsafe {
            let _ = PropVariantClear(&mut value);
        }
        Ok(result?)
    };
    let d = unsafe { e.GetDevice(&windows::core::HSTRING::from(source))? };
    if driver(&d).ok().as_deref() != Some("VBAudioVACWDM") {
        return Ok(source.to_owned());
    }
    let list = unsafe { e.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)? };
    let mut matches = Vec::new();
    for i in 0..unsafe { list.GetCount()? } {
        let d = unsafe { list.Item(i)? };
        if driver(&d).ok().as_deref() == Some("VBAudioVACWDM") {
            matches.push(device_id(&d)?);
        }
    }
    if matches.len() != 1 {
        return Err("VB-CABLE recording endpoint is missing or ambiguous".into());
    }
    let target = matches.remove(0);
    let (record_layout, rate) = crate::layout::read_endpoint(&target)?;
    if record_layout != *layout || rate != 48000 {
        // A stereo recording mix would irreversibly downmix the rear channels.
        // Match the source before opening the shared capture client.
        let format = wasapi::WaveFormat::new(
            16,
            16,
            &wasapi::SampleType::Int,
            48000,
            layout.channel_count,
            Some(layout.channel_mask),
        );
        let policy = policy_client()?;
        let wide: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let table = *(policy.as_raw() as *const *const PolicyVtable);
            ((*table).set_device_format)(
                policy.as_raw(),
                PCWSTR(wide.as_ptr()),
                format.as_waveformatex_ref(),
                format.as_waveformatex_ref(),
            )
            .ok()?;
        }
        let (actual, rate) = crate::layout::read_endpoint(&target)?;
        if actual != *layout || rate != 48000 {
            return Err("VB-CABLE recording format does not match the source channel mask".into());
        }
    }
    Ok(target)
}
#[implement(IMMNotificationClient)]
struct Notifications {
    changes: mpsc::Sender<(i32, Option<String>)>,
}
impl IMMNotificationClient_Impl for Notifications_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        id: &PCWSTR,
    ) -> windows::core::Result<()> {
        if flow == eRender {
            let text = if id.is_null() {
                None
            } else {
                unsafe { id.to_string() }.ok()
            };
            let _ = self.changes.send((role.0, text));
        }
        Ok(())
    }
}
fn restore(e: &IMMDeviceEnumerator, j: &mut Journal) -> Res<()> {
    let mut failures = Vec::new();
    for r in j.roles.clone() {
        if let Some(previous) = j.restore_to(r.role, current(e, r.role).as_deref()) {
            if !present(e, previous) {
                failures.push(format!("Previous output unavailable: {}", previous));
                continue;
            }
            if let Err(error) = set_default(previous, r.role) {
                failures.push(error.to_string());
                continue;
            }
        }
        if let Some(role) = j.roles.iter_mut().find(|v| v.role == r.role) {
            role.owned = false;
        }
    }
    save(j)?;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; ").into())
    }
}
fn reply(status: &Status) -> Res<()> {
    println!("{}", serde_json::to_string(status)?);
    std::io::stdout().flush()?;
    Ok(())
}
fn guardian() -> Res<()> {
    fs::create_dir_all(directory())?;
    // One owner per Windows user, including recovery. A second host cannot steal
    // the first host's journal or restore its output prematurely.
    let _lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .share_mode(0)
        .open(directory().join("default-output.lock"))?;
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    }
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }
    let _com = Com;
    let e: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    if journal_path().exists() {
        let mut old: Journal = serde_json::from_slice(&fs::read(journal_path())?)?;
        if old.version != 1 {
            return Err("Unsupported default-output recovery journal".into());
        }
        restore(&e, &mut old)?;
    }
    let Some(target) = find_virtual(&e)? else {
        reply(&Status::default())?;
        return Ok(());
    };
    // Windows may promote a newly installed endpoint on the first reboot.
    // Recover the installer's snapshot before acquiring our normal launch lease.
    let install_snapshot = directory().join("before-driver-install.json");
    if install_snapshot.exists() {
        let bytes = fs::read(&install_snapshot)?;
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
        let snapshot: serde_json::Value = serde_json::from_slice(bytes)?;
        let mut initial = Journal {
            version: 1,
            target: target.clone(),
            roles: Vec::new(),
        };
        if let Some(defaults) = snapshot.get("defaults").and_then(|v| v.as_array()) {
            for (role, id) in defaults.iter().take(3).enumerate() {
                if let Some(id) = id.as_str().filter(|id| *id != target) {
                    initial.roles.push(RoleLease {
                        role: role as i32,
                        previous: id.into(),
                        owned: true,
                    });
                }
            }
        }
        restore(&e, &mut initial)?;
        fs::remove_file(install_snapshot)?;
    }
    let roles: Vec<_> = [0, 1, 2]
        .into_iter()
        .filter_map(|role| {
            current(&e, role).map(|previous| RoleLease {
                role,
                previous,
                owned: false,
            })
        })
        .collect();
    let mut journal = Journal {
        version: 1,
        target: target.clone(),
        roles,
    };
    let mut status = Status {
        endpoint_id: Some(target.clone()),
        previous_output_id: current(&e, 0).filter(|id| id != &target),
        ..Default::default()
    };
    reply(&status)?;
    let (tx, commands) = mpsc::channel();
    thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(s) => {
                    if tx.send(s).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let (changes_tx, changes) = mpsc::channel();
    let notifications: IMMNotificationClient = Notifications {
        changes: changes_tx,
    }
    .into();
    unsafe {
        e.RegisterEndpointNotificationCallback(&notifications)?;
    }
    let run = (|| -> Res<()> {
        loop {
            let mut changed = false;
            while let Ok((role, id)) = changes.try_recv() {
                journal.observe(role, id.as_deref());
                changed = true;
            }
            if changed {
                save(&journal)?;
            }
            match commands.recv_timeout(Duration::from_millis(100)) {
                Ok(cmd) if cmd == "activate" && !status.active => {
                    // Do not overwrite a user choice made while the host prepared capture.
                    for r in &journal.roles {
                        if current(&e, r.role).as_deref() != Some(&r.previous) {
                            return Err(
                                "Windows output changed during startup; automatic switch cancelled"
                                    .into(),
                            );
                        }
                    }
                    for i in 0..journal.roles.len() {
                        if journal.roles[i].previous == target {
                            continue;
                        }
                        journal.roles[i].owned = true;
                        save(&journal)?; // persist BEFORE mutation
                        set_default(&target, journal.roles[i].role)?;
                    }
                    status.active = true;
                    reply(&status)?;
                }
                Ok(cmd) if cmd == "stop" => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                _ => {}
            }
        }
        Ok(())
    })();
    while let Ok((role, id)) = changes.try_recv() {
        journal.observe(role, id.as_deref());
    }
    let restoration = restore(&e, &mut journal);
    unsafe {
        let _ = e.UnregisterEndpointNotificationCallback(&notifications);
    }
    run?;
    restoration?;
    Ok(())
}
pub fn guardian_entry() -> bool {
    if let Some(path) = std::env::args().skip_while(|a| a != "--restore-install-output").nth(1) {
        let result = (|| -> Res<()> {
            let snapshot: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
            let defaults = snapshot["defaults"].as_array().ok_or("Invalid audio snapshot")?;
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()?; }
            let e: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
            let virtual_id = find_virtual(&e)?.ok_or("Virtual endpoint unavailable")?;
            for role in 0..3 {
                if let Some(previous) = defaults.get(role).and_then(|v| v.as_str()) {
                    // Undo only the driver's automatic takeover, never an intervening user choice.
                    if previous != virtual_id && current(&e, role as i32).as_deref() == Some(&virtual_id)
                        && present(&e, previous) {
                        set_default(previous, role as i32)?;
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = result { eprintln!("Restore installation output: {e}"); std::process::exit(1); }
        return true;
    }
    if std::env::args().any(|a| a == "--configure-vbcable") {
        let result = (|| -> Res<()> {
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()?; }
            let e: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
            let id = find_virtual(&e)?.ok_or("VB-CABLE speaker endpoint is not available; restart Windows after driver installation")?;
            let device = unsafe { e.GetDevice(&windows::core::HSTRING::from(&id))? };
            let store = unsafe { device.OpenPropertyStore(STGM_READ)? };
            let mut property = unsafe { store.GetValue(&PROPERTYKEY {
                fmtid: GUID::from_u128(0xa8b865dd_2e3d_4094_ad97_e593a70c75d6), pid: 8,
            })? };
            let driver = unsafe { PropVariantToStringAlloc(&property) }.map(|p| unsafe { take_string(p) });
            unsafe { PropVariantClear(&mut property)?; }
            if driver?.as_str() != "VBAudioVACWDM" { return Err("Refusing to configure a non VB-CABLE endpoint".into()); }
            let (before, rate) = crate::layout::read_endpoint(&id)?;
            let changed = before.channel_mask != 0x63f || before.channel_count != 8 || rate != 48000;
            if changed {
                let format = wasapi::WaveFormat::new(16,16,&wasapi::SampleType::Int,48000,8,Some(0x63f));
                let policy = policy_client()?;
                let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
                unsafe {
                    let table = *(policy.as_raw() as *const *const PolicyVtable);
                    ((*table).set_device_format)(policy.as_raw(), PCWSTR(wide.as_ptr()),
                        format.as_waveformatex_ref(), format.as_waveformatex_ref()).ok()?;
                }
            }
            let (after, rate) = crate::layout::read_endpoint(&id)?;
            if after.channel_mask != 0x63f || after.channel_count != 8 || rate != 48000 {
                return Err("VB-CABLE format verification failed".into());
            }
            println!("{}", serde_json::json!({"endpoint":id,"changed":changed,"layout":after,"sampleRate":rate}));
            Ok(())
        })();
        if let Err(e) = result { eprintln!("VB-CABLE configuration failed: {e}"); std::process::exit(1); }
        return true;
    }
    if std::env::args().any(|a| a == "--probe-virtual-audio") {
        let result = (|| -> Res<()> {
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            }
            let e: IMMDeviceEnumerator =
                unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
            let id = find_virtual(&e)?.ok_or("RoomWave Virtual Speakers is not installed")?;
            crate::virtual_probe::run(&id)
        })();
        if let Err(error) = result {
            eprintln!("Virtual audio probe failed: {error}");
            std::process::exit(1);
        }
        return true;
    }
    if std::env::args().any(|a| a == "--inspect-audio") {
        let result = (|| -> Res<serde_json::Value> {
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            }
            let e: IMMDeviceEnumerator =
                unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
            let devices = wasapi::DeviceEnumerator::new()?
                .get_device_collection(&wasapi::Direction::Render)?;
            let mut endpoints = Vec::new();
            for i in 0..devices.get_nbr_devices()? {
                let device = devices.get_device_at_index(i)?;
                let id = device.get_id()?;
                let format = crate::layout::read_endpoint(&id).ok();
                endpoints.push(
                    serde_json::json!({"id":id,"name":device.get_friendlyname()?,"format":format}),
                );
            }
            Ok(
                serde_json::json!({"virtualEndpoint":find_virtual(&e)?,"defaults":[current(&e,0),current(&e,1),current(&e,2)],"policyAvailable":policy_client().is_ok(),"endpoints":endpoints}),
            )
        })();
        match result {
            Ok(value) => println!("{value}"),
            Err(error) => eprintln!("{error}"),
        }
        return true;
    }
    if !std::env::args().any(|a| a == "--audio-guardian") {
        return false;
    }
    if let Err(e) = guardian() {
        let _ = reply(&Status {
            error: Some(e.to_string()),
            ..Default::default()
        });
        log::error!("Audio guardian: {e}");
    }
    true
}
struct Process {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}
pub struct DefaultOutput {
    process: Mutex<Option<Process>>,
    status: Mutex<Status>,
}
impl DefaultOutput {
    pub fn prepare() -> Self {
        let result = (|| -> Res<(Process, Status)> {
            let mut child = Command::new(std::env::current_exe()?)
                .arg("--audio-guardian")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .creation_flags(0x08000000)
                .spawn()?;
            let input = child.stdin.take().ok_or("No guardian stdin")?;
            let mut output = BufReader::new(child.stdout.take().ok_or("No guardian stdout")?);
            let mut line = String::new();
            output.read_line(&mut line)?;
            let status: Status = serde_json::from_str(&line)?;
            Ok((
                Process {
                    child,
                    input,
                    output,
                },
                status,
            ))
        })();
        match result {
            Ok((p, s)) => Self {
                process: Mutex::new(Some(p)),
                status: Mutex::new(s),
            },
            Err(e) => Self {
                process: Mutex::new(None),
                status: Mutex::new(Status {
                    error: Some(e.to_string()),
                    ..Default::default()
                }),
            },
        }
    }
    pub fn snapshot(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
    pub fn cancel(&self, message: &str) {
        self.status.lock().unwrap().error = Some(message.into());
        self.stop();
    }
    pub fn activate(&self) -> Result<(), String> {
        if self.snapshot().endpoint_id.is_none() {
            return Ok(());
        }
        let result = (|| -> Res<Status> {
            let mut guard = self.process.lock().unwrap();
            let p = guard.as_mut().ok_or("Guardian not running")?;
            writeln!(p.input, "activate")?;
            p.input.flush()?;
            let mut line = String::new();
            p.output.read_line(&mut line)?;
            Ok(serde_json::from_str(&line)?)
        })();
        match result {
            Ok(status) => {
                let error = status.error.clone();
                *self.status.lock().unwrap() = status;
                error.map_or(Ok(()), Err)
            }
            Err(e) => {
                let message = e.to_string();
                self.status.lock().unwrap().error = Some(message.clone());
                Err(message)
            }
        }
    }
    pub fn stop(&self) {
        if let Some(mut p) = self.process.lock().unwrap().take() {
            let _ = writeln!(p.input, "stop");
            drop(p.input);
            let _ = p.child.wait();
        }
        self.status.lock().unwrap().active = false;
    }
}
impl Drop for DefaultOutput {
    fn drop(&mut self) {
        self.stop();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn lease() -> Journal {
        Journal {
            version: 1,
            target: "virtual".into(),
            roles: vec![
                RoleLease {
                    role: 0,
                    previous: "speakers".into(),
                    owned: true,
                },
                RoleLease {
                    role: 2,
                    previous: "headset".into(),
                    owned: true,
                },
            ],
        }
    }
    #[test]
    fn restore_only_our_current_default() {
        let j = lease();
        assert_eq!(j.restore_to(0, Some("virtual")), Some("speakers"));
        assert_eq!(j.restore_to(0, Some("other")), None);
        assert_eq!(j.restore_to(0, None), None);
    }
    #[test]
    fn manual_change_away_and_back_is_respected() {
        let mut j = lease();
        j.observe(0, Some("other"));
        j.observe(0, Some("virtual"));
        assert_eq!(j.restore_to(0, Some("virtual")), None);
        assert_eq!(j.restore_to(2, Some("virtual")), Some("headset"));
    }
    #[test]
    fn own_notification_preserves_lease() {
        let mut j = lease();
        j.observe(0, Some("virtual"));
        assert_eq!(j.restore_to(0, Some("virtual")), Some("speakers"));
    }
    #[test]
    fn journal_round_trip_retains_released_role() {
        let mut j = lease();
        j.observe(2, Some("headset"));
        let saved = serde_json::to_vec(&j).unwrap();
        let recovered: Journal = serde_json::from_slice(&saved).unwrap();
        assert_eq!(recovered.restore_to(0, Some("virtual")), Some("speakers"));
        assert_eq!(recovered.restore_to(2, Some("virtual")), None);
    }
    #[test]
    #[ignore = "changes real Windows defaults; requires installed RoomWave and explicit physical endpoint"]
    fn windows_guardian_lifecycle() -> Res<()> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        let physical = std::env::var("ROOMWAVE_TEST_PHYSICAL_ID")?;
        let exe = std::env::var("ROOMWAVE_TEST_HOST")?;
        let e: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        let target = find_virtual(&e)?.ok_or("Missing virtual endpoint")?;
        assert!(physical != target && present(&e, &physical));
        struct Reset(String);
        impl Drop for Reset {
            fn drop(&mut self) {
                for r in [0, 1, 2] {
                    let _ = set_default(&self.0, r);
                }
            }
        }
        let _reset = Reset(physical.clone());
        for role in [0, 1, 2] {
            set_default(&physical, role)?;
        }
        for scenario in ["close", "crash_eof", "manual_away_back"] {
            let mut child = Command::new(&exe)
                .arg("--audio-guardian")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .creation_flags(0x08000000)
                .spawn()?;
            let mut input = child.stdin.take().unwrap();
            let mut output = BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            output.read_line(&mut line)?;
            let prepared: Status = serde_json::from_str(&line)?;
            assert!(prepared.error.is_none(), "{line}");
            writeln!(input, "activate")?;
            input.flush()?;
            line.clear();
            output.read_line(&mut line)?;
            let active: Status = serde_json::from_str(&line)?;
            assert!(active.active, "{line}");
            for role in [0, 1, 2] {
                assert_eq!(current(&e, role).as_deref(), Some(target.as_str()));
            }
            let mut manual_roles = Vec::new();
            if scenario == "manual_away_back" {
                set_default(&physical, 0)?;
                thread::sleep(Duration::from_millis(250));
                manual_roles = [0, 1, 2]
                    .into_iter()
                    .filter(|r| current(&e, *r).as_deref() == Some(physical.as_str()))
                    .collect();
                set_default(&target, 0)?;
                thread::sleep(Duration::from_millis(250));
            }
            if scenario != "crash_eof" {
                writeln!(input, "stop")?;
            }
            drop(input);
            child.wait()?;
            let expected = if scenario == "manual_away_back" {
                &target
            } else {
                &physical
            };
            assert_eq!(
                current(&e, 0).as_deref(),
                Some(expected.as_str()),
                "{scenario}"
            );
            for role in [1, 2] {
                let expected = if manual_roles.contains(&role) {
                    &target
                } else {
                    &physical
                };
                assert_eq!(
                    current(&e, role).as_deref(),
                    Some(expected.as_str()),
                    "{scenario}"
                );
            }
            for role in [0, 1, 2] {
                set_default(&physical, role)?;
            }
            println!("guardian scenario {scenario}: PASS");
        }
        Ok(())
    }
}
