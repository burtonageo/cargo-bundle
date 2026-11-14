//! Platform tables: `[package.metadata.bundle.linux]` (and `osx`/`macos`,
//! `windows`, `ios`, `android`) overrides shared keys for that platform and
//! holds the keys only that platform understands.

use super::{AndroidSettings, BundleSettings, IosSettings, LinuxSettings, OsxSettings, TargetOs};
use anyhow::Context;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::{Map, Value};

impl TargetOs {
    /// The name of this platform's table in the manifest; macOS is `osx`.
    fn table(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::MacOs => "osx",
            Self::Windows => "windows",
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }

    /// The platform a table name belongs to, `macos` being an alias of `osx`.
    fn from_table(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|os| os.table() == name || (*os == Self::MacOs && name == "macos"))
    }
}

/// The `windows` table has no keys of its own: all of them are shared ones.
#[derive(Deserialize)]
struct Windows {}

/// Read `value` as `T`, refusing any key `T` does not know, at any depth.
pub(super) fn strict<'de, T, D>(deserializer: D) -> crate::Result<T>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
    D::Error: Send + Sync + 'static,
{
    let mut unknown = Vec::new();
    let parsed = serde_ignored::deserialize(deserializer, |path| unknown.push(path.to_string()))?;
    anyhow::ensure!(
        unknown.is_empty(),
        "Unknown setting{} {}",
        if unknown.len() == 1 { "" } else { "s" },
        unknown.join(", ")
    );
    Ok(parsed)
}

/// Parse `package.metadata.bundle` for `target_os`. Every platform's table is
/// checked, not only the selected one, so a typo in a table that is not used
/// today fails now rather than on the release machine.
pub(super) fn parse(bundle: &Value, target_os: TargetOs) -> crate::Result<BundleSettings> {
    for os in TargetOs::ALL {
        read(bundle, os)?;
    }
    read(bundle, target_os)
}

fn read(bundle: &Value, target_os: TargetOs) -> crate::Result<BundleSettings> {
    let mut bundle = bundle.clone();
    select(&mut bundle, target_os)?;
    strict(bundle)
}

/// Flatten `bundle` for `target_os`: the platform table's shared keys replace
/// the root's, its own keys stay in a table of the same name, and the other
/// platforms' tables are dropped. `bin` and `example` tables get the same
/// treatment but do not inherit from the root.
fn select(bundle: &mut Value, target_os: TargetOs) -> crate::Result<()> {
    let root = bundle
        .as_object_mut()
        .context("package.metadata.bundle must be a table")?;
    for key in ["bin", "example"] {
        if let Some(Value::Object(named)) = root.get_mut(key) {
            for table in named.values_mut() {
                select(table, target_os)?;
            }
        }
    }
    if root.contains_key("osx") && root.contains_key("macos") {
        anyhow::bail!(
            "Use only one of [package.metadata.bundle.osx] or [package.metadata.bundle.macos]"
        );
    }
    if let Some(table) = root.remove("macos") {
        root.insert("osx".into(), table);
    }
    for os in TargetOs::ALL {
        let Some(table) = root.remove(os.table()) else {
            continue;
        };
        if os != target_os {
            continue;
        }
        let Value::Object(table) = table else {
            anyhow::bail!("[package.metadata.bundle.{}] must be a table", os.table());
        };
        let (own, shared) = split(os, table)?;
        for (key, value) in shared {
            anyhow::ensure!(
                !["bin", "example"].contains(&key.as_str()) && TargetOs::from_table(&key).is_none(),
                "Nested settings table `{key}` is not a platform override"
            );
            root.insert(key, value);
        }
        if !own.is_empty() {
            root.insert(os.table().into(), Value::Object(own));
        }
    }
    Ok(())
}

/// Split a platform table into the keys its settings read (its own) and the
/// rest, which shadow shared settings. The split is whatever the platform's
/// settings type leaves unread, so it cannot drift from the type.
fn split(
    os: TargetOs,
    table: Map<String, Value>,
) -> crate::Result<(Map<String, Value>, Map<String, Value>)> {
    fn unread<T: DeserializeOwned>(table: &Map<String, Value>) -> crate::Result<Vec<String>> {
        let mut unread = Vec::new();
        serde_ignored::deserialize::<_, _, T>(Value::Object(table.clone()), |path| {
            if let serde_ignored::Path::Map {
                parent: serde_ignored::Path::Root,
                key,
            } = path
            {
                unread.push(key);
            }
        })?;
        Ok(unread)
    }
    let unread = match os {
        TargetOs::Linux => unread::<LinuxSettings>(&table)?,
        TargetOs::MacOs => unread::<OsxSettings>(&table)?,
        TargetOs::Ios => unread::<IosSettings>(&table)?,
        TargetOs::Android => unread::<AndroidSettings>(&table)?,
        TargetOs::Windows => unread::<Windows>(&table)?,
    };
    Ok(table
        .into_iter()
        .partition(|(key, _)| !unread.contains(key)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(toml: &str, target_os: TargetOs) -> crate::Result<BundleSettings> {
        parse(&toml::from_str::<Value>(toml).unwrap(), target_os)
    }

    #[test]
    fn the_selected_table_shadows_the_root_and_keeps_its_own_keys() {
        let toml = r#"
            name = "Root"
            icon = ["root.png"]
            resources = ["data"]
            [linux]
            name = "Linux"
            resources = []
            use_terminal = true
            [bin.tool]
            name = "Tool"
            [bin.tool.linux]
            name = "Linux Tool"
        "#;
        let linux = settings(toml, TargetOs::Linux).unwrap();
        assert_eq!(linux.name.as_deref(), Some("Linux"));
        assert_eq!(linux.icon, Some(vec!["root.png".into()]));
        assert_eq!(linux.resources, Some(vec![]), "an empty array clears it");
        assert_eq!(linux.linux.unwrap().use_terminal, Some(true));
        assert_eq!(
            linux.bin.unwrap()["tool"].name.as_deref(),
            Some("Linux Tool")
        );

        let mac = settings(toml, TargetOs::MacOs).unwrap();
        assert_eq!(mac.name.as_deref(), Some("Root"));
        assert!(mac.linux.is_none(), "other platforms do not leak");
        assert_eq!(mac.bin.unwrap()["tool"].name.as_deref(), Some("Tool"));
    }

    #[test]
    fn keys_the_platform_does_not_read_are_shared_settings_and_must_exist() {
        let error = settings("[linux]\nframeworks = []\n", TargetOs::Linux).unwrap_err();
        assert!(error.to_string().contains("frameworks"), "{error}");
        // A platform's own keys do not leak into the shared ones.
        let linux = settings("[linux]\nuse_terminal = true\n", TargetOs::MacOs).unwrap();
        assert!(linux.linux.is_none());
    }

    #[test]
    fn unknown_keys_are_refused_at_any_depth() {
        for toml in [
            "bogus = 1",
            "[linux.localizations.fr]\nNmae = \"x\"",
            "[android]\nversio = 1",
            "[windows_signing]\ncertificate_path = \"a\"\nbogus = 1",
        ] {
            let error = settings(toml, TargetOs::Linux).unwrap_err();
            assert!(
                error.to_string().contains("Unknown setting"),
                "{toml}: {error}"
            );
        }
    }
}
