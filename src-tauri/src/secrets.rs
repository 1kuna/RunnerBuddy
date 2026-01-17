use crate::errors::Error;
use keyring::Entry;

const SERVICE: &str = "RunnerBuddy";
const LEGACY_ACCOUNT: &str = "pat";

fn entry(alias: &str) -> Result<Entry, Error> {
    Entry::new(SERVICE, &format!("pat:{alias}"))
        .map_err(|err| Error::Secrets(err.to_string()))
}

fn legacy_entry() -> Result<Entry, Error> {
    Entry::new(SERVICE, LEGACY_ACCOUNT).map_err(|err| Error::Secrets(err.to_string()))
}

pub fn save_pat(alias: &str, pat: &str) -> Result<(), Error> {
    entry(alias)?
        .set_password(pat)
        .map_err(|err| Error::Secrets(err.to_string()))
}

pub fn load_pat(alias: &str) -> Result<Option<String>, Error> {
    match entry(alias)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => {
            if alias == "default" {
                match legacy_entry()?.get_password() {
                    Ok(value) => Ok(Some(value)),
                    Err(keyring::Error::NoEntry) => Ok(None),
                    Err(err) => Err(Error::Secrets(err.to_string())),
                }
            } else {
                Ok(None)
            }
        }
        Err(err) => Err(Error::Secrets(err.to_string())),
    }
}

pub fn clear_pat(alias: &str) -> Result<(), Error> {
    match entry(alias)?.delete_password() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(Error::Secrets(err.to_string())),
    }
}
