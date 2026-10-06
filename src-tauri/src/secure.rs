//! macOS pieces of 芝麻开门: the keychain copy of the vault key, Touch ID, a concealed pasteboard
//! entry, and this build's identity. Unit tests swap all of it for in-memory fakes (no keychain
//! writes, no prompts).
//! Keychain: the app is ad-hoc signed, so macOS trusts an item only for the exact build that wrote it
//! (a newer build reading it gets a system prompt; deleting it fails with errSecInvalidOwnerEdit).
//! vault.rs therefore records which build saved the copy (`build_id`) and never reads another's.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bio {
    Ok,
    /// No match (macOS gives 3 tries per prompt) or biometry locked out.
    Failed,
    /// Cancelled, or "输入主密码" chosen.
    Cancelled,
    /// No Touch ID on this Mac right now (none, not enrolled, lid closed…).
    Unavailable,
}

#[cfg_attr(test, allow(dead_code))]
pub fn keychain_service() -> String {
    format!("{} 芝麻开门", crate::paths::data_dir_name())
}

pub use imp::*;

#[cfg(not(test))]
mod imp {
    use super::{keychain_service, Bio};
    use objc2::runtime::Bool;
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::{NSError, NSString};
    use objc2_local_authentication::{LAContext, LAPolicy};
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
    use std::sync::mpsc;
    use std::time::{Duration, UNIX_EPOCH};
    use zeroize::Zeroizing;

    const POLICY: LAPolicy = LAPolicy::DeviceOwnerAuthenticationWithBiometrics;
    // LAError codes
    const FAILED: isize = -1;
    const PASSCODE_NOT_SET: isize = -5;
    const NOT_AVAILABLE: isize = -6;
    const NOT_ENROLLED: isize = -7;
    const LOCKOUT: isize = -8;

    /// This executable's size + modification time: changes with every build, kept by `mv` on install.
    pub fn build_id() -> String {
        std::env::current_exe()
            .and_then(std::fs::metadata)
            .map(|m| {
                let t = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
                format!("{}-{t}", m.len())
            })
            .unwrap_or_default()
    }

    #[allow(unused_unsafe)]
    pub fn bio_available() -> bool {
        unsafe { LAContext::new().canEvaluatePolicy_error(POLICY).is_ok() }
    }

    /// Shows the system Touch ID prompt and waits for it (call off the main thread).
    #[allow(unused_unsafe)]
    pub fn bio_verify(reason: &str) -> Bio {
        unsafe {
            let ctx = LAContext::new();
            if ctx.canEvaluatePolicy_error(POLICY).is_err() {
                return Bio::Unavailable;
            }
            ctx.setLocalizedFallbackTitle(Some(&NSString::from_str("输入主密码")));
            let (tx, rx) = mpsc::channel();
            let reply = block2::RcBlock::new(move |ok: Bool, e: *mut NSError| {
                let code = if e.is_null() { 0 } else { (*e).code() };
                let _ = tx.send((ok.as_bool(), code));
            });
            ctx.evaluatePolicy_localizedReason_reply(POLICY, &NSString::from_str(reason), &reply);
            match rx.recv() {
                Ok((true, _)) => Bio::Ok,
                Ok((false, FAILED | LOCKOUT)) => Bio::Failed,
                Ok((false, PASSCODE_NOT_SET | NOT_AVAILABLE | NOT_ENROLLED)) => Bio::Unavailable,
                _ => Bio::Cancelled,
            }
        }
    }

    pub fn key_save(account: &str, key: &[u8]) -> Result<(), String> {
        set_generic_password(&keychain_service(), account, key).map_err(|e| e.to_string())
    }

    pub fn key_load(account: &str) -> Result<Zeroizing<Vec<u8>>, String> {
        get_generic_password(&keychain_service(), account).map(Zeroizing::new).map_err(|e| e.to_string())
    }

    /// Only for copies this build wrote (others can't be deleted, see module doc).
    pub fn key_delete(account: &str) {
        let _ = delete_generic_password(&keychain_service(), account);
    }

    /// Puts `text` on the pasteboard marked concealed (clipboard managers skip it); cleared after
    /// `clear_after` unless something else was copied meanwhile.
    #[allow(unused_unsafe)]
    pub fn copy_concealed(text: &str, clear_after: Duration) {
        let count = unsafe {
            let pb = NSPasteboard::generalPasteboard();
            pb.clearContents();
            pb.setString_forType(&NSString::from_str(text), NSPasteboardTypeString);
            pb.setString_forType(&NSString::from_str(""), &NSString::from_str("org.nspasteboard.ConcealedType"));
            pb.changeCount()
        };
        std::thread::spawn(move || {
            std::thread::sleep(clear_after);
            unsafe {
                let pb = NSPasteboard::generalPasteboard();
                if pb.changeCount() == count {
                    pb.clearContents();
                }
            }
        });
    }
}

#[cfg(test)]
mod imp {
    pub use super::fake::*;
}

#[cfg(test)]
pub mod fake {
    use super::Bio;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::time::Duration;
    use zeroize::Zeroizing;

    thread_local! {
        pub static AVAILABLE: Cell<bool> = const { Cell::new(true) };
        pub static BUILD: RefCell<String> = RefCell::new("build-1".into());
        pub static KEYCHAIN: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
        pub static CLIPBOARD: RefCell<String> = const { RefCell::new(String::new()) };
    }

    pub fn build_id() -> String {
        BUILD.with_borrow(|b| b.clone())
    }
    pub fn bio_available() -> bool {
        AVAILABLE.get()
    }
    pub fn bio_verify(_reason: &str) -> Bio {
        if AVAILABLE.get() {
            Bio::Ok
        } else {
            Bio::Unavailable
        }
    }
    pub fn key_save(account: &str, key: &[u8]) -> Result<(), String> {
        KEYCHAIN.with_borrow_mut(|k| k.insert(account.into(), key.to_vec()));
        Ok(())
    }
    pub fn key_load(account: &str) -> Result<Zeroizing<Vec<u8>>, String> {
        KEYCHAIN.with_borrow(|k| k.get(account).cloned().map(Zeroizing::new).ok_or_else(|| "not found".into()))
    }
    pub fn key_delete(account: &str) {
        KEYCHAIN.with_borrow_mut(|k| k.remove(account));
    }
    pub fn copy_concealed(text: &str, _clear_after: Duration) {
        CLIPBOARD.with_borrow_mut(|c| *c = text.into());
    }
}
