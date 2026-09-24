use std::path::PathBuf;

pub fn settings_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir).join("RoomWave")
}
pub fn log_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is unavailable")?)
        .join("RoomWave").join("logs"))
}
