//! Reading networks from WifiConfigStore.xml (text XML, not ABX) to recreate them with
//! `cmd wifi add-network` on Android 11+ (backup-android.md point 8).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum WifiSecurity {
    Open,
    Owe,
    Wep,
    Wpa2,
    Wpa3,
    /// 802.1X: needs certificates and credentials that are not in the file.
    Enterprise,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub security: WifiSecurity,
    pub password: Option<String>,
    pub hidden: bool,
}

impl WifiNetwork {
    /// `cmd wifi add-network` arguments, or `None` when it cannot be recreated this way.
    pub fn add_network_args(&self) -> Option<Vec<String>> {
        let ty = match self.security {
            WifiSecurity::Open => "open",
            WifiSecurity::Owe => "owe",
            WifiSecurity::Wep => "wep",
            WifiSecurity::Wpa2 => "wpa2",
            WifiSecurity::Wpa3 => "wpa3",
            WifiSecurity::Enterprise => return None,
        };
        let mut args = vec![self.ssid.clone(), ty.to_string()];
        match (self.security, &self.password) {
            (WifiSecurity::Open | WifiSecurity::Owe, _) => {}
            (_, Some(p)) => args.push(p.clone()),
            (_, None) => return None,
        }
        if self.hidden {
            args.push("-h".into());
        }
        Some(args)
    }
}

pub fn parse(xml: &str) -> Vec<WifiNetwork> {
    xml.split("<Network>").skip(1).filter_map(|block| parse_network(block.split("</Network>").next()?)).collect()
}

fn parse_network(block: &str) -> Option<WifiNetwork> {
    // SSIDs and passphrases are stored quoted ("&quot;name&quot;"); unquoted hex values are raw
    // bytes or pre-computed keys, which add-network cannot take.
    let ssid = quoted(&string_value(block, "SSID")?)?;
    let password = string_value(block, "PreSharedKey").and_then(|p| quoted(&p)).or_else(|| wep_key(block));
    let hidden = block.contains("name=\"HiddenSSID\" value=\"true\"");
    let types: Vec<u32> = block
        .split("name=\"SecurityType\" value=\"")
        .skip(1)
        .filter_map(|r| r.split('"').next()?.parse().ok())
        .collect();
    let security = if !types.is_empty() {
        // Types: 0 open, 1 WEP, 2 PSK, 3 EAP, 4 SAE, 5 EAP Suite B, 6 OWE. A WPA2/WPA3
        // transition network lists both; wpa2 joins either.
        if types.contains(&2) {
            WifiSecurity::Wpa2
        } else if types.contains(&4) {
            WifiSecurity::Wpa3
        } else if types.contains(&3) || types.contains(&5) {
            WifiSecurity::Enterprise
        } else if types.contains(&1) {
            WifiSecurity::Wep
        } else if types.contains(&6) {
            WifiSecurity::Owe
        } else {
            WifiSecurity::Open
        }
    } else if password.is_some() {
        WifiSecurity::Wpa2
    } else {
        WifiSecurity::Open
    };
    Some(WifiNetwork { ssid, security, password, hidden })
}

/// Text of `<string name="NAME">…</string>`, unescaped.
fn string_value(block: &str, name: &str) -> Option<String> {
    let start = block.find(&format!("<string name=\"{name}\">"))? + name.len() + 16;
    let end = block[start..].find("</string>")? + start;
    Some(unescape(&block[start..end]))
}

fn wep_key(block: &str) -> Option<String> {
    let start = block.find("<string-array name=\"WEPKeys\"")?;
    let item = block[start..].split("<item value=\"").nth(1)?.split('"').next()?;
    quoted(&unescape(item))
}

fn quoted(s: &str) -> Option<String> {
    s.strip_prefix('"')?.strip_suffix('"').map(str::to_string)
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version='1.0' encoding='utf-8' standalone='yes' ?>
<WifiConfigStoreData>
<NetworkList>
<Network>
<WifiConfiguration>
<string name="ConfigKey">&quot;Casa &amp; Co&quot;WPA_PSK</string>
<string name="SSID">&quot;Casa &amp; Co&quot;</string>
<string name="PreSharedKey">&quot;secreto&quot;&quot;x&quot;</string>
<boolean name="HiddenSSID" value="true" />
<SecurityParamsList>
<SecurityParams>
<int name="SecurityType" value="2" />
</SecurityParams>
<SecurityParams>
<int name="SecurityType" value="4" />
</SecurityParams>
</SecurityParamsList>
</WifiConfiguration>
</Network>
<Network>
<WifiConfiguration>
<string name="SSID">&quot;Libre&quot;</string>
<null name="PreSharedKey" />
<boolean name="HiddenSSID" value="false" />
<SecurityParamsList><SecurityParams><int name="SecurityType" value="0" /></SecurityParams></SecurityParamsList>
</WifiConfiguration>
</Network>
<Network>
<WifiConfiguration>
<string name="SSID">&quot;Oficina&quot;</string>
<SecurityParamsList><SecurityParams><int name="SecurityType" value="3" /></SecurityParams></SecurityParamsList>
</WifiConfiguration>
</Network>
<Network>
<WifiConfiguration>
<string name="SSID">e382bfe382a4</string>
</WifiConfiguration>
</Network>
</NetworkList>
</WifiConfigStoreData>"#;

    #[test]
    fn parses_networks() {
        let nets = parse(XML);
        assert_eq!(nets.len(), 3, "the hex SSID is skipped");
        assert_eq!(
            nets[0],
            WifiNetwork { ssid: "Casa & Co".into(), security: WifiSecurity::Wpa2, password: Some("secreto\"\"x".into()), hidden: true }
        );
        assert_eq!(nets[0].add_network_args().unwrap(), ["Casa & Co", "wpa2", "secreto\"\"x", "-h"]);
        assert_eq!(nets[1].security, WifiSecurity::Open);
        assert_eq!(nets[1].add_network_args().unwrap(), ["Libre", "open"]);
        assert_eq!(nets[2].security, WifiSecurity::Enterprise);
        assert_eq!(nets[2].add_network_args(), None);
    }
}
