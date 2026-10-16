//! "Auto-Start with Windows": a value under the Run key (per user or for all users).

use std::path::Path;
use winreg::RegKey;
use winreg::enums::*;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

fn root(all_users: bool) -> RegKey {
    RegKey::predef(if all_users { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER })
}

pub fn is_enabled(value_name: &str, all_users: bool) -> bool {
    root(all_users).open_subkey(RUN_KEY).and_then(|k| k.get_value::<String, _>(value_name)).is_ok()
}

/// Add or remove the start-up entry. All-users needs administrator rights.
pub fn set_enabled(value_name: &str, enable: bool, all_users: bool, exe: &Path) -> Result<(), String> {
    let key = root(all_users).create_subkey_with_flags(RUN_KEY, KEY_SET_VALUE | KEY_QUERY_VALUE).map_err(|e| {
        if all_users { format!("Could not write the all-users entry (administrator rights needed): {e}") } else { e.to_string() }
    })?;
    if enable {
        key.0.set_value(value_name, &format!("\"{}\"", exe.display())).map_err(|e| e.to_string())
    } else {
        match key.0.delete_value(value_name) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_user_entry_round_trip() {
        let name = "HSnap-test-entry";
        let exe = Path::new(r"C:\Program Files\HSnap\hsnap.exe");
        set_enabled(name, true, false, exe).unwrap();
        assert!(is_enabled(name, false));
        let value: String = RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_KEY).unwrap().get_value(name).unwrap();
        assert_eq!(value, r#""C:\Program Files\HSnap\hsnap.exe""#);
        set_enabled(name, false, false, exe).unwrap();
        assert!(!is_enabled(name, false));
        set_enabled(name, false, false, exe).unwrap(); // removing twice is fine
    }
}
