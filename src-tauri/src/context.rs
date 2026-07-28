use crate::fs_ops;
use std::path::{Path, PathBuf};

pub struct AppContext {
    data_dir: PathBuf,
    home_dir: PathBuf,
}

impl AppContext {
    pub fn new(data_dir: PathBuf, home_dir: PathBuf) -> Self {
        Self { data_dir, home_dir }
    }

    pub fn from_env() -> Result<Self, String> {
        let home_dir = fs_ops::home_dir();
        let data_dir = std::env::var_os("OMS_DATA_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir.join(".oh-my-skills-cent"));
        Ok(Self { data_dir, home_dir })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // from_env reads process-wide environment variables; serialize the tests so
    // they cannot observe each other's OMS_DATA_DIR.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn from_env_prefers_oms_data_dir() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let custom = PathBuf::from("custom-oms-data");
        std::env::set_var("OMS_DATA_DIR", &custom);

        let ctx = AppContext::from_env().expect("from_env");

        std::env::remove_var("OMS_DATA_DIR");
        assert_eq!(ctx.data_dir(), custom.as_path());
        assert_eq!(ctx.home_dir(), fs_ops::home_dir().as_path());
    }

    #[test]
    fn from_env_defaults_to_home_subdir() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        std::env::remove_var("OMS_DATA_DIR");

        let ctx = AppContext::from_env().expect("from_env");

        let home = fs_ops::home_dir();
        assert_eq!(ctx.data_dir(), home.join(".oh-my-skills-cent").as_path());
        assert_eq!(ctx.home_dir(), home.as_path());
    }
}
