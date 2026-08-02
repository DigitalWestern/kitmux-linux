use kitmux_model::preview_macos_state_file;
use std::env;
use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    let source = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: kitmux-import-preview MACOS_STATE_JSON LINUX_HOME")?;
    let linux_home = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: kitmux-import-preview MACOS_STATE_JSON LINUX_HOME")?;
    if arguments.next().is_some() {
        return Err("usage: kitmux-import-preview MACOS_STATE_JSON LINUX_HOME".into());
    }
    let preview = preview_macos_state_file(&source, &linux_home)?;
    println!("{}", serde_json::to_string_pretty(&preview)?);
    Ok(())
}
