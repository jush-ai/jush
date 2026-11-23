//! The device's apps that open a page's links to them, such as
//! `chat://client/authorize?token=…`: which links are for an app, which app
//! the device opens one with, and handing one to the system to open.
//!
//! Chrome hands such a link to the system only after its own dialog, which a
//! Chrome without a window never shows, so Jush asks the user instead
//! (`tabs::apps`) and opens the link here once the user chose the device.
//! What counts as a link to an app follows Chrome's
//! `external_protocol_handler.cc`: a scheme Chrome does not load itself, and
//! none it never hands to a system.
//!
//! A link reaches the system escaped as Chrome escapes it, as one argument
//! of the system's own opener, never through a shell.

/// The schemes Chrome loads itself, which no app opens.
const BROWSER_SCHEMES: &[&str] = &[
    "about",
    "blob",
    "chrome",
    "chrome-error",
    "chrome-extension",
    "chrome-search",
    "chrome-untrusted",
    "data",
    "devtools",
    "file",
    "filesystem",
    "http",
    "https",
    "isolated-app",
    "javascript",
    "view-source",
    "ws",
    "wss",
];

/// The schemes Chrome never hands to a system, as its `kDeniedSchemes` lists
/// them: each could run a program or reach a disk through the system's
/// handler.
const DENIED_SCHEMES: &[&str] = &[
    "afp",
    "applescript",
    "data",
    "disk",
    "disks",
    "file",
    "hcp",
    "ie.http",
    "javascript",
    "mk",
    "ms-help",
    "nntp",
    "res",
    "shell",
    "vbscript",
    "view-source",
    "vnd.ms.radio",
];

/// The scheme of `url`, lowercased, when it has a valid one.
fn scheme(url: &str) -> Option<String> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then(|| scheme.to_ascii_lowercase())
}

/// Whether `url` is a link to an app, which Chrome would hand to the system
/// to open. A scheme of one letter is a Windows drive, such as `C:/file`.
pub fn is_app_link(url: &str) -> bool {
    scheme(url).is_some_and(|scheme| {
        scheme.len() > 1
            && !BROWSER_SCHEMES.contains(&scheme.as_str())
            && !DENIED_SCHEMES.contains(&scheme.as_str())
    })
}

/// `url` as the system receives it, escaped as Chrome's
/// `EscapeExternalHandlerValue` escapes a link before it hands it on: every
/// byte but the letters, the digits, the characters a URL reserves and
/// ``!'()*-._~#[]`` is percent-encoded, an escape already in place is kept.
/// A quote, a space or a caret thus never reaches an app's command line,
/// where it would end the link and start an argument of the page's.
fn escaped(url: &str) -> String {
    let bytes = url.as_bytes();
    let mut escaped = String::with_capacity(url.len());
    for (index, &byte) in bytes.iter().enumerate() {
        let kept_escape = byte == b'%'
            && bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit);
        let plain = byte.is_ascii_alphanumeric() || b";/?:@&=+$,!'()*-._~#[]".contains(&byte);
        if kept_escape || plain {
            escaped.push(char::from(byte));
        } else {
            escaped.push_str(&format!("%{byte:02X}"));
        }
    }
    escaped
}

/// The name of the app the device opens `url` with; none when it has none,
/// or the system cannot say. It blocks while the system looks.
pub fn app_for(url: &str) -> Option<String> {
    system::app_for(&escaped(url)).filter(|name| !name.is_empty())
}

/// Hands `url`, a link to an app, to the device's system to open, and says
/// whether the system took it.
pub async fn open(url: &str) -> std::io::Result<()> {
    if !is_app_link(url) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the link is not one an app opens",
        ));
    }
    system::open(escaped(url)).await
}

/// Runs the system's opener `program` with `url` and says how it ended. An
/// opener can wait for the app it started, as `xdg-open` does without a
/// desktop, so nothing else waits for it.
#[cfg(not(windows))]
async fn run_opener(program: &str, url: &str) -> std::io::Result<()> {
    let status = tokio::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("{program} could not open the link ({status})")))
    }
}

#[cfg(target_os = "macos")]
mod system {
    //! Launch Services: the app the system's `open` would start for a link,
    //! by the name Finder shows for it.

    use std::ffi::{c_char, c_void};

    type CFTypeRef = *const c_void;

    const UTF8: u32 = 0x0800_0100;
    const ALL_ROLES: u32 = 0xFFFF_FFFF;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: CFTypeRef);
        fn CFURLCreateWithBytes(
            allocator: CFTypeRef,
            bytes: *const u8,
            length: isize,
            encoding: u32,
            base: CFTypeRef,
        ) -> CFTypeRef;
        fn CFStringGetLength(string: CFTypeRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
        fn CFStringGetCString(string: CFTypeRef, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
    }

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSCopyDefaultApplicationURLForURL(url: CFTypeRef, roles: u32, error: *mut CFTypeRef) -> CFTypeRef;
        fn LSCopyDisplayNameForURL(url: CFTypeRef, name: *mut CFTypeRef) -> i32;
    }

    /// A Core Foundation object this module created or copied, released
    /// when dropped.
    struct Owned(CFTypeRef);

    impl Owned {
        fn new(object: CFTypeRef) -> Option<Self> {
            // `then`, not `then_some`: a null object must never be wrapped,
            // since dropping the wrapper releases it.
            (!object.is_null()).then(|| Self(object))
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the object was created or copied by this module, which
            // owns this one reference to it.
            unsafe { CFRelease(self.0) }
        }
    }

    /// The text of a Core Foundation string.
    fn text(string: &Owned) -> Option<String> {
        // SAFETY: `string` is a live CFString; the buffer is as large as the
        // string's UTF-8 can be, with its terminating zero.
        unsafe {
            let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(string.0), UTF8) + 1;
            let mut buffer = vec![0u8; usize::try_from(size).ok()?];
            if CFStringGetCString(string.0, buffer.as_mut_ptr().cast(), size, UTF8) == 0 {
                return None;
            }
            let end = buffer.iter().position(|&byte| byte == 0)?;
            buffer.truncate(end);
            String::from_utf8(buffer).ok()
        }
    }

    pub(super) fn app_for(url: &str) -> Option<String> {
        // SAFETY: each call gets live objects this function owns, and every
        // object it is handed is released once (`Owned`).
        unsafe {
            let link = Owned::new(CFURLCreateWithBytes(
                std::ptr::null(),
                url.as_ptr(),
                isize::try_from(url.len()).ok()?,
                UTF8,
                std::ptr::null(),
            ))?;
            let app = Owned::new(LSCopyDefaultApplicationURLForURL(
                link.0,
                ALL_ROLES,
                std::ptr::null_mut(),
            ))?;
            let mut name: CFTypeRef = std::ptr::null();
            if LSCopyDisplayNameForURL(app.0, &mut name) != 0 {
                return None;
            }
            let name = text(&Owned::new(name)?)?;
            // The name keeps `.app` where Finder shows extensions.
            Some(name.strip_suffix(".app").unwrap_or(&name).to_owned())
        }
    }

    pub(super) async fn open(url: String) -> std::io::Result<()> {
        super::run_opener("/usr/bin/open", &url).await
    }
}

#[cfg(windows)]
mod system {
    //! The shell's association of the link's scheme, as Explorer opens a
    //! link with it.

    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::{
        System::{
            Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize},
            SystemInformation::GetSystemDirectoryW,
        },
        UI::{
            Shell::{ASSOCF_IS_PROTOCOL, ASSOCSTR_FRIENDLYAPPNAME, AssocQueryStringW, ShellExecuteW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };

    /// `text` as a zero-terminated UTF-16 string.
    fn wide(text: &str) -> Vec<u16> {
        std::ffi::OsStr::new(text).encode_wide().chain([0]).collect()
    }

    pub(super) fn app_for(url: &str) -> Option<String> {
        let (scheme, _) = url.split_once(':')?;
        let scheme = wide(scheme);
        let mut length = 0u32;
        // SAFETY: the scheme is zero-terminated; the first call only measures
        // the name, the second writes it into a buffer of that length.
        unsafe {
            AssocQueryStringW(
                ASSOCF_IS_PROTOCOL,
                ASSOCSTR_FRIENDLYAPPNAME,
                scheme.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                &mut length,
            );
            if length == 0 {
                return None;
            }
            let mut name = vec![0u16; length as usize];
            if AssocQueryStringW(
                ASSOCF_IS_PROTOCOL,
                ASSOCSTR_FRIENDLYAPPNAME,
                scheme.as_ptr(),
                std::ptr::null(),
                name.as_mut_ptr(),
                &mut length,
            ) != 0
            {
                return None;
            }
            let end = name.iter().position(|&unit| unit == 0).unwrap_or(name.len());
            String::from_utf16(&name[..end]).ok()
        }
    }

    /// The system folder, `C:\Windows\System32`, as zero-terminated UTF-16.
    fn system_folder() -> Vec<u16> {
        // SAFETY: the first call measures the path with its terminating
        // zero, the second writes it into a buffer of that length.
        unsafe {
            let length = GetSystemDirectoryW(std::ptr::null_mut(), 0);
            let mut folder = vec![0u16; length as usize];
            let written = GetSystemDirectoryW(folder.as_mut_ptr(), length);
            folder.truncate(written as usize);
            folder.push(0);
            folder
        }
    }

    pub(super) async fn open(url: String) -> std::io::Result<()> {
        tokio::task::spawn_blocking(move || execute(&url))
            .await
            .unwrap_or_else(|error| Err(std::io::Error::other(error)))
    }

    /// Opens `url` as Chrome's `platform_util` does: quoted, so the link is
    /// one argument whatever it holds, and from the system folder, so the
    /// app neither inherits nor holds the runner's folder.
    fn execute(url: &str) -> std::io::Result<()> {
        let (verb, url, folder) = (wide("open"), wide(&format!("\"{url}\"")), system_folder());
        /// What `ShellExecuteW` answers above when it succeeded.
        const SUCCEEDED_ABOVE: isize = 32;
        // SAFETY: COM is started on this thread for the call, as the shell's
        // handlers may use it, and ended after; the verb, the link and the
        // folder are zero-terminated and outlive the call.
        let result = unsafe {
            let started = CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32);
            let result = ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                url.as_ptr(),
                std::ptr::null(),
                folder.as_ptr(),
                SW_SHOWNORMAL,
            );
            if started >= 0 {
                CoUninitialize();
            }
            result
        };
        if result as isize > SUCCEEDED_ABOVE {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "Windows could not open the link (ShellExecute answered {})",
                result as isize
            )))
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod system {
    //! The desktop's default handler of the link's scheme, as `xdg-open`
    //! opens a link with it, by the name its desktop entry gives.

    use std::path::PathBuf;

    /// The folders desktop entries lie in, the user's first.
    fn application_folders() -> Vec<PathBuf> {
        let home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
        let system = std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|dirs| !dirs.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
        home.into_iter()
            .chain(system.split(':').map(PathBuf::from))
            .map(|folder| folder.join("applications"))
            .collect()
    }

    /// The `Name` of the desktop entry `id`, or the entry's own name when
    /// none is found.
    fn entry_name(id: &str) -> String {
        for folder in application_folders() {
            let Ok(entry) = std::fs::read_to_string(folder.join(id)) else {
                continue;
            };
            let mut in_entry = false;
            for line in entry.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_entry = line == "[Desktop Entry]";
                } else if in_entry && let Some(name) = line.strip_prefix("Name=") {
                    return name.trim().to_owned();
                }
            }
        }
        id.strip_suffix(".desktop").unwrap_or(id).to_owned()
    }

    pub(super) fn app_for(url: &str) -> Option<String> {
        let (scheme, _) = url.split_once(':')?;
        let output = std::process::Command::new("xdg-mime")
            .args(["query", "default", &format!("x-scheme-handler/{scheme}")])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        let id = String::from_utf8(output.stdout).ok()?;
        let id = id.trim();
        (output.status.success() && !id.is_empty()).then(|| entry_name(id))
    }

    pub(super) async fn open(url: String) -> std::io::Result<()> {
        super::run_opener("xdg-open", &url).await
    }
}
