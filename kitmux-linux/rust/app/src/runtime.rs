use std::env;
use std::ffi::{CStr, CString, OsString, c_char};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;

use crate::diagnostic;

pub(crate) struct RuntimeBundle {
    pub(crate) kitty_src: CString,
    pub(crate) libkitty_py: CString,
    pub(crate) python_home: CString,
    pub(crate) config: Option<CString>,
}

impl RuntimeBundle {
    pub(crate) fn discover() -> Result<Self, &'static str> {
        let environment = (
            env::var_os("KITTY_SRC"),
            env::var_os("LIBKITTY_PY"),
            env::var_os("PYTHONHOME"),
        );
        let (kitty_src, libkitty_py, python_home, config) = match environment {
            (Some(kitty), Some(glue), Some(python)) => (
                PathBuf::from(kitty),
                PathBuf::from(glue),
                PathBuf::from(python),
                env::var_os("LIBKITTY_TEST_CONFIG").map(PathBuf::from),
            ),
            (None, None, None) => {
                let executable = env::current_exe().map_err(|_| "executable")?;
                let root = executable
                    .parent()
                    .and_then(Path::parent)
                    .ok_or("runtime-root")?
                    .to_owned();
                (
                    root.clone(),
                    root.join("libkitty_py"),
                    root.clone(),
                    Some(root.join("etc/kitty.conf")),
                )
            }
            _ => return Err("partial-runtime-environment"),
        };
        if !kitty_src.join("kitty/fast_data_types.so").is_file() {
            return Err("kitty-runtime");
        }
        if !libkitty_py.join("glue.py").is_file() {
            return Err("libkitty-glue");
        }
        if !python_home.is_dir() {
            return Err("python-runtime");
        }
        Ok(Self {
            kitty_src: path_cstring(&kitty_src)?,
            libkitty_py: path_cstring(&libkitty_py)?,
            python_home: path_cstring(&python_home)?,
            config: config
                .filter(|path| path.is_file())
                .map(|path| path_cstring(&path))
                .transpose()?,
        })
    }
}

pub(crate) fn path_cstring(path: &Path) -> Result<CString, &'static str> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| "path-nul")
}

pub(crate) struct Account {
    pub(crate) home: PathBuf,
    pub(crate) shell: CString,
}

pub(crate) fn account() -> Account {
    unsafe {
        let uid = libc::geteuid();
        let mut entry: libc::passwd = std::mem::zeroed();
        let mut result = ptr::null_mut();
        let mut buffer = vec![0_u8; 16 * 1024];
        if libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        ) == 0
            && !result.is_null()
        {
            let home = c_path(entry.pw_dir).filter(|path| path.is_absolute());
            let shell =
                c_path(entry.pw_shell).filter(|path| path.is_absolute() && is_executable(path));
            if let (Some(home), Some(shell)) = (home, shell)
                && let Ok(shell) = path_cstring(&shell)
            {
                return Account { home, shell };
            }
        }
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    Account {
        home,
        shell: CString::new("/bin/sh").unwrap(),
    }
}

pub(crate) fn valid_restored_cwd(path: &Path) -> bool {
    path.is_absolute()
        && path.is_dir()
        && path_cstring(path)
            .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::X_OK) == 0 })
}

pub(crate) unsafe fn c_path(value: *const c_char) -> Option<PathBuf> {
    if value.is_null() {
        return None;
    }
    let bytes = unsafe { CStr::from_ptr(value) }.to_bytes();
    (!bytes.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

pub(crate) fn is_executable(path: &Path) -> bool {
    let Ok(path) = path_cstring(path) else {
        return false;
    };
    unsafe { libc::access(path.as_ptr(), libc::X_OK) == 0 }
}

pub(crate) fn session_environment(account: &Account, ssh: bool) -> Vec<OsString> {
    let mut values = vec![
        OsString::from(format!("SHELL={}", account.shell.to_string_lossy())),
        OsString::from(format!("HOME={}", account.home.display())),
        OsString::from(format!(
            "PATH={}",
            env::var_os("PATH")
                .unwrap_or_else(|| OsString::from("/usr/bin:/bin"))
                .to_string_lossy()
        )),
        OsString::from("COLORTERM=truecolor"),
        OsString::from("TERM=xterm-256color"),
    ];
    for (key, value) in env::vars_os() {
        let key = key.to_string_lossy();
        if key == "USER" || key == "LOGNAME" || key == "LANG" || key.starts_with("LC_") {
            values.push(OsString::from(format!("{key}={}", value.to_string_lossy())));
        }
    }
    if ssh {
        if let Some(agent) = env::var_os("SSH_AUTH_SOCK").filter(|value| !value.is_empty()) {
            values.push(OsString::from(format!(
                "SSH_AUTH_SOCK={}",
                agent.to_string_lossy()
            )));
            diagnostic("ssh_agent", &[("available", "true".to_owned())]);
        } else {
            diagnostic("ssh_agent", &[("available", "false".to_owned())]);
        }
    }
    values
}

pub(crate) fn owned_c_string(value: *mut c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned();
    unsafe { libc::free(value.cast()) };
    Some(text)
}

pub(crate) fn c_buffer(value: &[c_char]) -> Option<String> {
    let end = value.iter().position(|byte| *byte == 0)?;
    let bytes = &value[..end];
    Some(
        String::from_utf8_lossy(unsafe {
            std::slice::from_raw_parts(bytes.as_ptr().cast::<u8>(), bytes.len())
        })
        .into_owned(),
    )
}

pub(crate) fn kitty_stage_error(stage: &str, error: &[c_char]) -> String {
    let detail = c_buffer(error)
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| "libkitty returned no diagnostic".to_owned());
    format!("{stage}: {detail}")
}
