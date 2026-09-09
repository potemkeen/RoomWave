use mdns_sd::{DaemonEvent, ResolvedService, ServiceDaemon, ServiceEvent};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

pub const SERVICE_TYPE: &str = "_roomwave._udp.local.";
const PROTOCOL_VERSION: u16 = 4;
const RESERVED_PORT: u16 = 47800;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub device_id: String,
    pub device_name: String,
    pub ip_address: String,
    pub port: u16,
    pub protocol_version: u16,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverySnapshot {
    pub devices: Vec<Device>,
    pub discovering: bool,
    pub error: Option<String>,
}

#[derive(Default)]
struct DeviceList {
    // Track DNS instances independently: a goodbye for one must not remove another.
    instances: BTreeMap<String, Device>,
}

impl DeviceList {
    fn update(&mut self, fullname: String, device: Device) {
        let fullname = fullname.to_lowercase();
        if self.instances.get(&fullname) != Some(&device) {
            log::info!(
                "device discovered: {} ({}) id={}",
                device.device_name,
                device.ip_address,
                device.device_id
            );
        }
        self.instances.insert(fullname, device);
    }

    fn remove(&mut self, fullname: &str) {
        if let Some(device) = self.instances.remove(&fullname.to_lowercase()) {
            log::info!(
                "device removed: {} ({})",
                device.device_name,
                device.device_id
            );
        }
    }

    fn devices(&self) -> Vec<Device> {
        let mut unique = BTreeMap::new();
        for device in self.instances.values() {
            unique.entry(&device.device_id).or_insert(device.clone());
        }
        let mut devices: Vec<_> = unique.into_values().collect();
        devices.sort_by(|a, b| (&a.device_name, &a.device_id).cmp(&(&b.device_name, &b.device_id)));
        devices
    }
}

enum Command {
    Refresh,
    Stop,
}

pub struct Discovery {
    snapshot: Arc<Mutex<DiscoverySnapshot>>,
    commands: mpsc::SyncSender<Command>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Discovery {
    pub fn start() -> Result<Self, String> {
        let snapshot = Arc::new(Mutex::new(DiscoverySnapshot::default()));
        let (commands, receiver) = mpsc::sync_channel(1);
        let worker_snapshot = Arc::clone(&snapshot);
        let worker = thread::Builder::new()
            .name("roomwave-discovery".into())
            .spawn(move || run(worker_snapshot, receiver))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            snapshot,
            commands,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn snapshot(&self) -> Result<DiscoverySnapshot, String> {
        self.snapshot
            .lock()
            .map(|s| s.clone())
            .map_err(|e| e.to_string())
    }

    pub fn refresh(&self) -> Result<(), String> {
        match self.commands.try_send(Command::Refresh) {
            Ok(()) | Err(mpsc::TrySendError::Full(_)) => Ok(()),
            Err(e) => Err(format!("Discovery worker unavailable: {e}")),
        }
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        if let Err(error) = self.commands.send(Command::Stop) {
            log::error!("discovery shutdown error: {error}");
        }
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                if worker.join().is_err() {
                    log::error!("discovery worker panicked");
                }
            }
        }
    }
}

fn publish(
    shared: &Mutex<DiscoverySnapshot>,
    list: &DeviceList,
    discovering: bool,
    error: &Option<String>,
) {
    match shared.lock() {
        Ok(mut snapshot) => {
            *snapshot = DiscoverySnapshot {
                devices: list.devices(),
                discovering,
                error: error.clone(),
            }
        }
        Err(e) => log::error!("discovery state error: {e}"),
    }
}

fn shutdown(daemon: &ServiceDaemon) {
    match daemon.shutdown() {
        Ok(done) => {
            if let Err(e) = done.recv_timeout(Duration::from_secs(2)) {
                log::error!("discovery shutdown error: {e}");
            }
        }
        Err(e) => log::error!("discovery shutdown error: {e}"),
    }
}

fn run(shared: Arc<Mutex<DiscoverySnapshot>>, commands: mpsc::Receiver<Command>) {
    'restart: loop {
        let mut list = DeviceList::default();
        let mut error = None;
        publish(&shared, &list, false, &error);
        let daemon = match ServiceDaemon::new() {
            Ok(daemon) => daemon,
            Err(e) => {
                error = Some(e.to_string());
                log::error!("discovery error: {e}");
                publish(&shared, &list, false, &error);
                match commands.recv() {
                    Ok(Command::Refresh) => continue,
                    _ => return,
                }
            }
        };
        let receivers = daemon
            .monitor()
            .and_then(|monitor| daemon.browse(SERVICE_TYPE).map(|events| (monitor, events)));
        let (monitor, events) = match receivers {
            Ok(receivers) => receivers,
            Err(e) => {
                log::error!("discovery error: {e}");
                error = Some(e.to_string());
                publish(&shared, &list, false, &error);
                shutdown(&daemon);
                match commands.recv() {
                    Ok(Command::Refresh) => continue,
                    _ => return,
                }
            }
        };
        log::info!("discovery started: {SERVICE_TYPE}");
        let mut discovering = true;
        loop {
            match commands.recv_timeout(Duration::from_millis(100)) {
                Ok(Command::Refresh) => {
                    log::info!("discovery refresh requested");
                    shutdown(&daemon);
                    // A new daemon also clears the DNS cache; no stale events cross this boundary.
                    continue 'restart;
                }
                Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    shutdown(&daemon);
                    return;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            for event in events.try_iter() {
                match event {
                    ServiceEvent::ServiceResolved(service) => match parse_device(&service) {
                        Ok(device) => list.update(service.get_fullname().into(), device),
                        Err(e) => {
                            list.remove(service.get_fullname());
                            log::warn!("discovery ignored {}: {e}", service.get_fullname());
                        }
                    },
                    ServiceEvent::ServiceRemoved(_, fullname) => list.remove(&fullname),
                    ServiceEvent::SearchStopped(_) => {
                        discovering = false;
                        error = Some("mDNS search stopped; use Refresh to restart".into());
                        log::error!("discovery error: search stopped");
                    }
                    _ => {}
                }
            }
            for event in monitor.try_iter() {
                if let DaemonEvent::Error(e) = event {
                    log::error!("discovery error: {e}");
                    error = Some(e.to_string());
                }
            }
            if events.is_disconnected() || monitor.is_disconnected() {
                discovering = false;
                if error.is_none() {
                    error = Some("mDNS daemon disconnected; use Refresh to restart".into());
                    log::error!("discovery error: daemon disconnected");
                }
                list.instances.clear();
            }
            publish(&shared, &list, discovering, &error);
        }
    }
}

fn parse_device(service: &ResolvedService) -> Result<Device, String> {
    if !service.ty_domain.eq_ignore_ascii_case(SERVICE_TYPE) {
        return Err("unexpected service type".into());
    }
    let text = |key: &str| -> Result<String, String> {
        let bytes = service
            .get_property_val(key)
            .flatten()
            .ok_or_else(|| format!("missing {key}"))?;
        let value = std::str::from_utf8(bytes).map_err(|_| format!("invalid UTF-8 in {key}"))?;
        if value.trim().is_empty() || value.len() > 200 || value.chars().any(char::is_control) {
            return Err(format!("invalid {key}"));
        }
        Ok(value.to_owned())
    };
    let version = text("protocolVersion")?;
    if version != PROTOCOL_VERSION.to_string() {
        return Err(format!("unsupported protocolVersion {version}"));
    }
    if service.get_port() != RESERVED_PORT {
        return Err("unexpected audio port".into());
    }
    let ip_address = service
        .get_addresses()
        .iter()
        .filter(|ip| {
            !ip.is_loopback()
                && !ip.to_ip_addr().is_unspecified()
                && !ip.to_ip_addr().is_multicast()
        })
        .min_by_key(|ip| (!ip.is_ipv4(), ip.to_string()))
        .ok_or("no usable LAN address")?
        .to_string();
    Ok(Device {
        device_id: text("deviceId")?,
        device_name: text("deviceName")?,
        ip_address,
        port: service.get_port(),
        protocol_version: PROTOCOL_VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdns_sd::ServiceInfo;

    fn service(properties: &[(&str, &str)], addresses: &str) -> ResolvedService {
        ServiceInfo::new(
            SERVICE_TYPE,
            "Test",
            "test.local.",
            addresses,
            RESERVED_PORT,
            properties,
        )
        .unwrap()
        .as_resolved_service()
    }

    #[test]
    fn resolves_metadata_and_prefers_ipv4() {
        let record = service(
            &[
                ("deviceId", "test-id"),
                ("deviceName", "Телефон"),
                ("protocolVersion", "4"),
            ],
            "fd00::2,192.168.1.25",
        );
        let device = parse_device(&record).unwrap();
        assert_eq!(device.device_name, "Телефон");
        assert_eq!(device.ip_address, "192.168.1.25");
        assert_eq!(device.port, 47800);
    }

    #[test]
    fn rejects_missing_metadata_unsupported_versions_and_non_lan_addresses() {
        let record = service(
            &[("deviceId", "test-id"), ("protocolVersion", "4")],
            "192.168.1.25",
        );
        assert!(parse_device(&record).unwrap_err().contains("deviceName"));
        let record = service(
            &[
                ("deviceId", "test-id"),
                ("deviceName", "Phone"),
                ("protocolVersion", "99"),
            ],
            "192.168.1.25",
        );
        assert!(parse_device(&record).unwrap_err().contains("unsupported"));
        let record = service(
            &[
                ("deviceId", "test-id"),
                ("deviceName", "Phone"),
                ("protocolVersion", "4"),
            ],
            "127.0.0.1,::1",
        );
        assert!(parse_device(&record).unwrap_err().contains("address"));
    }

    #[test]
    fn rejects_bad_txt_and_wrong_port() {
        let mut record = service(
            &[
                ("deviceId", "test-id"),
                ("deviceName", "Phone\nInjected"),
                ("protocolVersion", "4"),
            ],
            "192.168.1.25",
        );
        assert!(parse_device(&record).unwrap_err().contains("deviceName"));
        record.port = 0;
        assert!(parse_device(&record).unwrap_err().contains("port"));
    }

    #[test]
    #[ignore = "Uses real LAN multicast; run explicitly with a connected network"]
    fn lan_discovery_refresh_goodbye_and_reappearance() {
        use std::time::Instant;
        fn wait_for(discovery: &Discovery, id: &str, present: bool) {
            let until = Instant::now() + Duration::from_secs(20);
            loop {
                let snapshot = discovery.snapshot().unwrap();
                assert!(
                    snapshot.error.is_none(),
                    "Discovery error: {:?}",
                    snapshot.error
                );
                if snapshot.devices.iter().any(|device| device.device_id == id) == present {
                    return;
                }
                assert!(
                    Instant::now() < until,
                    "Timed out waiting for device present={present}"
                );
                thread::sleep(Duration::from_millis(100));
            }
        }

        let discovery = Discovery::start().unwrap();
        let publisher = ServiceDaemon::new().unwrap();
        let id = format!("roomwave-test-{}", std::process::id());
        let properties = [
            ("deviceId", id.as_str()),
            ("deviceName", "RoomWave test receiver"),
            ("protocolVersion", "4"),
        ];
        let record = ServiceInfo::new(
            SERVICE_TYPE,
            &id,
            &format!("{id}.local."),
            "",
            RESERVED_PORT,
            &properties[..],
        )
        .unwrap()
        .enable_addr_auto();
        publisher.register(record.clone()).unwrap();
        wait_for(&discovery, &id, true);
        discovery.refresh().unwrap();
        // Allow the worker to process Refresh before checking rediscovery.
        thread::sleep(Duration::from_secs(3));
        wait_for(&discovery, &id, true);
        publisher
            .unregister(record.get_fullname())
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        wait_for(&discovery, &id, false);
        publisher.register(record.clone()).unwrap();
        wait_for(&discovery, &id, true);
        publisher
            .unregister(record.get_fullname())
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        shutdown(&publisher);
    }

    fn device(id: &str, address: &str) -> Device {
        Device {
            device_id: id.into(),
            device_name: "Test phone".into(),
            ip_address: address.into(),
            port: RESERVED_PORT,
            protocol_version: 1,
        }
    }

    #[test]
    fn duplicate_instances_survive_one_goodbye_and_reappear() {
        let mut list = DeviceList::default();
        list.update("Phone-A.local.".into(), device("same-id", "192.168.1.2"));
        list.update("phone-b.local.".into(), device("same-id", "192.168.1.2"));
        assert_eq!(list.devices().len(), 1);
        list.remove("PHONE-A.LOCAL.");
        assert_eq!(list.devices().len(), 1);
        list.remove("phone-b.local.");
        assert!(list.devices().is_empty());
        list.update("phone-a.local.".into(), device("same-id", "192.168.1.3"));
        assert_eq!(list.devices()[0].ip_address, "192.168.1.3");
    }
}
