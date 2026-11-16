use anyhow::{Context, Result, bail};
use ibkit_gateway::{Config, Credentials, settings::FIELDS};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub enum Kind {
    Text,
    Number,
    Choice(&'static [&'static str]),
    Secret,
}

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub section: usize,
    pub value: String,
    pub kind: Kind,
}

pub struct Form {
    pub fields: Vec<Field>,
    pub section: usize,
    pub selected: usize,
    pub dirty: bool,
    original: Option<Vec<u8>>,
}

impl Form {
    pub fn load(path: &Path) -> Result<Self> {
        let original = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("Cannot open the selected config. Select a readable TOML file with --config or correct its permissions."),
        };
        let mut config: Config = if let Some(bytes) = &original {
            toml::from_str(std::str::from_utf8(bytes).context("The selected config is not UTF-8. Keep the original file and correct its encoding before you open the TUI.")?)
                .context("The selected config is not valid TOML. Keep the original file and correct its keys and value types before you open the TUI.")?
        } else {
            Config::default()
        };
        // Preset the Gateway launcher and native agent found on this machine
        // when the config still has the built-in defaults.
        ibkit_gateway::settings::fill_paths(&mut config);
        let values = toml::Value::try_from(config)?.try_into::<toml::Table>()?;
        let fields = FIELDS
            .iter()
            .enumerate()
            .flat_map(|(section, rows)| {
                rows.iter()
                    .map(move |&(key, label, help)| (section, key, label, help))
            })
            .map(|(section, key, label, help)| {
                let kind = match key {
                    "_username" | "_password" | "_totp_secret" => Kind::Secret,
                    "_remember" => Kind::Choice(&["false", "true"]),
                    "trading_mode" => Kind::Choice(&["paper", "live"]),
                    "existing_session" => Kind::Choice(&["refuse", "takeover"]),
                    "incoming_connection" => Kind::Choice(&["manual", "accept", "reject"]),
                    "region" => Kind::Choice(&["america", "europe", "asia"]),
                    _ => match values.get(key) {
                        Some(toml::Value::Boolean(_)) => Kind::Choice(&["false", "true"]),
                        Some(toml::Value::Integer(_)) => Kind::Number,
                        None if matches!(key, "java_heap_mb" | "master_client_id") => Kind::Number,
                        _ => Kind::Text,
                    },
                };
                let value = if key == "_remember" {
                    "false".into()
                } else {
                    values
                        .get(key)
                        .map(|value| match value {
                            toml::Value::String(text) => text.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default()
                };
                Field {
                    key,
                    label,
                    help,
                    section,
                    value,
                    kind,
                }
            })
            .collect();
        Ok(Self {
            fields,
            section: 0,
            selected: 0,
            dirty: original.is_none(),
            original,
        })
    }

    pub fn rows(&self) -> Vec<usize> {
        self.fields
            .iter()
            .enumerate()
            .filter_map(|(i, field)| (field.section == self.section).then_some(i))
            .collect()
    }
    pub fn current(&self) -> usize {
        self.rows()[self.selected]
    }

    /// The value of the field `key`.
    pub fn value(&self, key: &str) -> &str {
        &self
            .fields
            .iter()
            .find(|field| field.key == key)
            .expect("every key in FIELDS has a field")
            .value
    }

    /// The entered Mobile Authenticator secret, checked; empty when none.
    pub fn totp_secret(&self) -> Result<&str> {
        let secret = self.value("_totp_secret").trim();
        if !secret.is_empty() {
            ibkit_gateway::totp::decode_secret(secret)?;
        }
        Ok(secret)
    }

    pub fn credentials(&self) -> Result<Option<Credentials>> {
        let username = self.value("_username");
        let password = self.value("_password");
        if username.is_empty() && password.is_empty() {
            return Ok(None);
        }
        if [username, password]
            .iter()
            .any(|value| value.is_empty() || value.contains(['\n', '\r', '\0']))
        {
            bail!(
                "You entered incomplete or multiline credentials. Enter one nonempty username and password, or clear both fields to use the configured sources."
            );
        }
        Ok(Some(Credentials {
            username: username.into(),
            password: password.into(),
        }))
    }

    pub fn config(&self) -> Result<Config> {
        let mut table = toml::Table::new();
        for field in &self.fields {
            if field.key.starts_with('_') {
                continue;
            }
            if let Some(value) = ibkit_gateway::settings::parse_value(field.key, &field.value)
                .with_context(|| format!("Edit the {} field and try again.", field.label))?
            {
                table.insert(field.key.into(), value);
            }
        }
        let config: Config = table.try_into().context("The settings contain an invalid value type. Use the field help to correct ports, paths, and limits.")?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&mut self, path: &Path) -> Result<()> {
        let mut config = self.config()?;
        let credentials = self.credentials()?;
        let totp = self.totp_secret()?.to_owned();
        let current = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        if current != self.original {
            bail!(
                "The config changed outside this TUI. Close and reopen the TUI before saving; the other file has been preserved."
            );
        }
        let parent = path.parent().context(
            "The config has no parent. Select a config filename in a writable directory.",
        )?;
        fs::create_dir_all(parent)?;
        let saved = |file: &Option<PathBuf>, value: &str| {
            file.as_ref()
                .is_some_and(|path| fs::read(path).is_ok_and(|bytes| bytes == value.as_bytes()))
        };
        let unsaved = credentials.as_ref().is_some_and(|credentials| {
            !(saved(&config.username_file, &credentials.username)
                && saved(&config.password_file, &credentials.password))
        }) || !totp.is_empty() && !saved(&config.totp_secret_file, &totp);
        let private = if self.value("_remember") == "true" && unsaved {
            let dir =
                ibkit_gateway::platform::private_tempdir(".ibkit-credentials-", Some(parent))?;
            write_credentials(dir.path(), credentials.as_ref(), &totp, &mut config)?;
            Some(dir)
        } else {
            None
        };
        let text = toml::to_string_pretty(&config)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(text.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(path).context("Cannot save settings. Select a writable config path and correct its permissions; the previous file is preserved.")?;
        if let Some(dir) = private {
            let _ = dir.keep();
        }
        self.original = Some(text.into_bytes());
        for (key, value) in [
            ("username_file", &config.username_file),
            ("password_file", &config.password_file),
            ("totp_secret_file", &config.totp_secret_file),
        ] {
            if let Some(value) = value {
                self.fields
                    .iter_mut()
                    .find(|field| field.key == key)
                    .unwrap()
                    .value = value.display().to_string();
            }
        }
        self.dirty = false;
        Ok(())
    }

    pub fn session_config(&self, directory: &Path) -> Result<Config> {
        let mut config = self.config()?;
        config.resolve_paths()?;
        let credentials = self.credentials()?;
        let totp = self.totp_secret()?;
        if credentials.is_some() || !totp.is_empty() {
            write_credentials(directory, credentials.as_ref(), totp, &mut config)?;
        }
        Ok(config)
    }

    pub fn redact(&self, text: &str) -> String {
        Credentials {
            username: self.value("_username").into(),
            password: self.value("_password").into(),
        }
        .redact(text)
    }
}

/// Write each entered secret to a private 0600 file in `directory` and
/// point the configuration at it. Empty or absent secrets are skipped.
fn write_credentials(
    directory: &Path,
    credentials: Option<&Credentials>,
    totp: &str,
    config: &mut Config,
) -> Result<()> {
    let (username, password) = credentials.map_or(("", ""), |credentials| {
        (credentials.username.as_str(), credentials.password.as_str())
    });
    for (name, value, target) in [
        ("username", username, &mut config.username_file),
        ("password", password, &mut config.password_file),
        ("totp_secret", totp, &mut config.totp_secret_file),
    ] {
        if value.is_empty() {
            continue;
        }
        let path: PathBuf = directory.join(name);
        ibkit_gateway::gateway::write_private(&path, value.as_bytes())?;
        *target = Some(path);
    }
    Ok(())
}
