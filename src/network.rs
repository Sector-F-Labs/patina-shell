//! Wi-Fi through NetworkManager on the system D-Bus, for the bar's Wi-Fi
//! icon and the Wi-Fi menu (ADR-0022). The calls wait for D-Bus, so run
//! them off the UI thread.
//!
//! On Ubuntu, NetworkManager's polkit rules let the active session list
//! networks, connect and switch Wi-Fi off and on, and let members of `sudo`
//! save new networks, all without a password prompt.

use std::collections::HashMap;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const SETTINGS: &str = "org.freedesktop.NetworkManager.Settings";
const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";

/// NM_DEVICE_TYPE_WIFI.
const TYPE_WIFI: u32 = 2;
/// NM_802_11_AP_FLAGS_PRIVACY.
const PRIVACY: u32 = 0x1;
/// NM_802_11_AP_SEC_KEY_MGMT_* in WpaFlags and RsnFlags.
const KEY_PSK: u32 = 0x100;
const KEY_802_1X: u32 = 0x200;
const KEY_SAE: u32 = 0x400;

/// How a network is secured, as far as joining it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Open,
    /// WPA/WPA2 personal: a password.
    Psk,
    /// WPA3 personal: a password.
    Sae,
    /// Enterprise (a login, certificates) or WEP: not joinable from the
    /// menu.
    Other,
}

/// What an access point's flags say about joining it.
pub fn security(flags: u32, wpa: u32, rsn: u32) -> Security {
    let keys = wpa | rsn;
    if keys & KEY_802_1X != 0 {
        Security::Other
    } else if keys & KEY_PSK != 0 {
        Security::Psk
    } else if keys & KEY_SAE != 0 {
        Security::Sae
    } else if flags & PRIVACY != 0 {
        // Privacy with no WPA: WEP.
        Security::Other
    } else {
        Security::Open
    }
}

/// A network in range: the strongest access point of each name.
#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub ssid: String,
    /// 0 to 100.
    pub strength: u8,
    pub security: Security,
    /// The one we're on.
    pub active: bool,
    /// NetworkManager has a saved connection for it.
    pub saved: bool,
    pub access_point: OwnedObjectPath,
}

/// Wi-Fi as a whole.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wifi {
    pub enabled: bool,
    /// Whether there's a Wi-Fi device at all.
    pub present: bool,
    /// Best first: the active network, then saved ones, then by strength.
    pub networks: Vec<Network>,
}

impl Wifi {
    /// The active network's strength, for the bar's icon.
    pub fn signal(&self) -> Option<u8> {
        self.networks.iter().find(|n| n.active).map(|n| n.strength)
    }
}

pub struct NetworkManager {
    bus: Connection,
}

impl NetworkManager {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            bus: Connection::system().map_err(|e| e.to_string())?,
        })
    }

    fn proxy<'a>(&'a self, path: &'a str, interface: &'a str) -> Result<Proxy<'a>, String> {
        Proxy::new(&self.bus, NM, path, interface).map_err(|e| e.to_string())
    }

    fn device(&self) -> Option<OwnedObjectPath> {
        let nm = self.proxy(NM_PATH, NM).ok()?;
        let devices: Vec<OwnedObjectPath> = nm.call("GetDevices", &()).ok()?;
        devices.into_iter().find(|d| {
            self.proxy(d.as_str(), DEVICE)
                .ok()
                .and_then(|p| p.get_property::<u32>("DeviceType").ok())
                == Some(TYPE_WIFI)
        })
    }

    /// Wi-Fi's state and the networks in range.
    pub fn wifi(&self) -> Result<Wifi, String> {
        let nm = self.proxy(NM_PATH, NM)?;
        let enabled: bool = nm
            .get_property("WirelessEnabled")
            .map_err(|e| e.to_string())?;
        let Some(device) = self.device() else {
            return Ok(Wifi {
                enabled,
                present: false,
                networks: Vec::new(),
            });
        };
        let wireless = self.proxy(device.as_str(), WIRELESS)?;
        let active: OwnedObjectPath = wireless
            .get_property("ActiveAccessPoint")
            .map_err(|e| e.to_string())?;
        let points: Vec<OwnedObjectPath> = wireless
            .call("GetAllAccessPoints", &())
            .map_err(|e| e.to_string())?;
        let saved = self.saved_ssids();

        let mut by_name: HashMap<String, Network> = HashMap::new();
        for path in points {
            let Ok(ap) = self.proxy(path.as_str(), AP) else {
                continue;
            };
            let ssid: Vec<u8> = ap.get_property("Ssid").unwrap_or_default();
            let ssid = String::from_utf8_lossy(&ssid).into_owned();
            if ssid.is_empty() {
                continue; // hidden
            }
            let strength: u8 = ap.get_property("Strength").unwrap_or(0);
            let flags: u32 = ap.get_property("Flags").unwrap_or(0);
            let wpa: u32 = ap.get_property("WpaFlags").unwrap_or(0);
            let rsn: u32 = ap.get_property("RsnFlags").unwrap_or(0);
            let network = Network {
                saved: saved.contains(&ssid),
                active: path == active,
                security: security(flags, wpa, rsn),
                strength,
                access_point: path.clone(),
                ssid: ssid.clone(),
            };
            match by_name.get(&ssid) {
                Some(other) if other.active || (!network.active && other.strength >= strength) => {}
                _ => {
                    by_name.insert(ssid, network);
                }
            }
        }
        let mut networks: Vec<Network> = by_name.into_values().collect();
        sort(&mut networks);
        Ok(Wifi {
            enabled,
            present: true,
            networks,
        })
    }

    /// The names of the Wi-Fi networks NetworkManager has saved.
    fn saved_ssids(&self) -> Vec<String> {
        self.saved_connections()
            .into_iter()
            .map(|(_, ssid)| ssid)
            .collect()
    }

    fn saved_connections(&self) -> Vec<(OwnedObjectPath, String)> {
        let Ok(settings) = self.proxy(SETTINGS_PATH, SETTINGS) else {
            return Vec::new();
        };
        let Ok(paths) = settings.call::<_, _, Vec<OwnedObjectPath>>("ListConnections", &()) else {
            return Vec::new();
        };
        paths
            .into_iter()
            .filter_map(|path| {
                let connection = self.proxy(path.as_str(), CONNECTION).ok()?;
                let all: HashMap<String, HashMap<String, OwnedValue>> =
                    connection.call("GetSettings", &()).ok()?;
                let ssid = all.get("802-11-wireless")?.get("ssid")?;
                let bytes: Vec<u8> = ssid.try_clone().ok()?.try_into().ok()?;
                Some((path.clone(), String::from_utf8_lossy(&bytes).into_owned()))
            })
            .collect()
    }

    /// For the bar's icon, cheaply: `None` without a Wi-Fi device,
    /// `Some(None)` when it's off or not connected, else the signal.
    pub fn signal(&self) -> Option<Option<u8>> {
        let device = self.device()?;
        let enabled: bool = self
            .proxy(NM_PATH, NM)
            .ok()?
            .get_property("WirelessEnabled")
            .ok()?;
        if !enabled {
            return Some(None);
        }
        let active: OwnedObjectPath = self
            .proxy(device.as_str(), WIRELESS)
            .ok()?
            .get_property("ActiveAccessPoint")
            .ok()?;
        if active.as_str() == "/" {
            return Some(None);
        }
        let strength: u8 = self
            .proxy(active.as_str(), AP)
            .ok()?
            .get_property("Strength")
            .ok()?;
        Some(Some(strength))
    }

    /// Asks the device to look for networks; NetworkManager limits how
    /// often, so a refusal is fine.
    pub fn scan(&self) {
        if let Some(device) = self.device()
            && let Ok(wireless) = self.proxy(device.as_str(), WIRELESS)
        {
            let options: HashMap<&str, Value> = HashMap::new();
            let _ = wireless.call::<_, _, ()>("RequestScan", &(options,));
        }
    }

    /// Joins `network`: with its saved connection if there is one,
    /// otherwise a new one, with `password` for a secured network.
    pub fn connect(&self, network: &Network, password: Option<&str>) -> Result<(), String> {
        let device = self.device().ok_or("no Wi-Fi device")?;
        let nm = self.proxy(NM_PATH, NM)?;
        if let Some((saved, _)) = self
            .saved_connections()
            .into_iter()
            .find(|(_, ssid)| *ssid == network.ssid)
        {
            return nm
                .call::<_, _, OwnedObjectPath>(
                    "ActivateConnection",
                    &(&saved, &device, &network.access_point),
                )
                .map(|_| ())
                .map_err(|e| e.to_string());
        }
        let mut settings: HashMap<&str, HashMap<&str, Value>> = HashMap::new();
        let key_mgmt = match network.security {
            Security::Open => None,
            Security::Psk => Some("wpa-psk"),
            Security::Sae => Some("sae"),
            Security::Other => return Err("this kind of network needs nmtui".into()),
        };
        if let Some(key_mgmt) = key_mgmt {
            let password = password.ok_or("a password is needed")?;
            let mut security: HashMap<&str, Value> = HashMap::new();
            security.insert("key-mgmt", Value::from(key_mgmt));
            security.insert("psk", Value::from(password));
            settings.insert("802-11-wireless-security", security);
        }
        nm.call::<_, _, (OwnedObjectPath, OwnedObjectPath)>(
            "AddAndActivateConnection",
            &(settings, &device, &network.access_point),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// Leaves the current network.
    pub fn disconnect(&self) -> Result<(), String> {
        let device = self.device().ok_or("no Wi-Fi device")?;
        self.proxy(device.as_str(), DEVICE)?
            .call::<_, _, ()>("Disconnect", &())
            .map_err(|e| e.to_string())
    }

    pub fn set_enabled(&self, on: bool) -> Result<(), String> {
        self.proxy(NM_PATH, NM)?
            .set_property("WirelessEnabled", on)
            .map_err(|e| e.to_string())
    }
}

/// The active network first, then saved ones, then the strongest.
pub fn sort(networks: &mut [Network]) {
    networks.sort_by(|a, b| {
        (b.active, b.saved, b.strength, &a.ssid).cmp(&(a.active, a.saved, a.strength, &b.ssid))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_from_flags() {
        assert_eq!(security(0, 0, 0), Security::Open);
        // nifl's network: privacy, RSN with PSK.
        assert_eq!(security(3, 0, 392), Security::Psk);
        assert_eq!(security(1, 0, 0x400 | 0x8), Security::Sae);
        assert_eq!(security(1, 0, 0x200 | 0x8), Security::Other);
        assert_eq!(security(1, 0, 0), Security::Other, "WEP");
    }

    fn network(ssid: &str, strength: u8, active: bool, saved: bool) -> Network {
        Network {
            ssid: ssid.into(),
            strength,
            security: Security::Psk,
            active,
            saved,
            access_point: OwnedObjectPath::try_from("/x").unwrap(),
        }
    }

    #[test]
    fn active_then_saved_then_strongest() {
        let mut n = vec![
            network("strong", 90, false, false),
            network("home", 40, true, true),
            network("cafe", 30, false, true),
            network("weak", 10, false, false),
        ];
        sort(&mut n);
        let names: Vec<&str> = n.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(names, ["home", "cafe", "strong", "weak"]);
    }
}
