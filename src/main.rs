use directories::ProjectDirs;
use eframe::egui;
use lighty_launcher::java::jre_downloader::{find_java_binary, jre_download};
use lighty_launcher::launch::{Installer, LaunchArguments};
use lighty_launcher::loaders::{Version, VersionMetaData};
use lighty_launcher::prelude::*;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command as TokioCommand;

const MINECRAFT_VERSIONS: &[&str] = &[
    "26.1.2", "26.1.1", "26.1", "1.21.11", "1.21.10", "1.21.9", "1.21.8", "1.21.7", "1.21.6",
    "1.21.5", "1.21.4", "1.21.3", "1.21.2", "1.21.1", "1.21", "1.20.6", "1.20.5", "1.20.4",
    "1.20.3", "1.20.2", "1.20.1", "1.20", "1.19.4", "1.19.3", "1.19.2", "1.19.1", "1.19", "1.18.2",
    "1.18.1", "1.18", "1.17.1", "1.17", "1.16.5", "1.16.4", "1.16.3", "1.16.2", "1.16.1", "1.16",
    "1.15.2", "1.15.1", "1.15", "1.14.4", "1.13.2", "1.12.2", "1.8.9", "1.7.10",
];

const MICROSOFT_SESSION_CACHE: &str = "microsoft_session.json";
const CRASH_LOG_FILE: &str = "crash.log";
const MOD_MANIFEST_FILE: &str = "fusion_mods_manifest.json";
const SHADER_MANIFEST_FILE: &str = "fusion_shaderpacks_manifest.json";
const LAUNCHER_CONFIG_FILE: &str = "fusion_launcher_config.json";
const EXPECTED_LAUNCH_SECONDS: u64 = 67;
const MAX_MOD_PROFILES: u8 = 5;
const APP_LOGO_PNG: &[u8] =
    include_bytes!("../logo.png");

static LAUNCHER_DIR: Lazy<ProjectDirs> = Lazy::new(|| {
    ProjectDirs::from("com", "fusion", "fusion-launcher")
        .expect("Failed to locate target OS system folder structures.")
});

fn microsoft_session_cache_path() -> PathBuf {
    LAUNCHER_DIR
        .data_dir()
        .to_path_buf()
        .join(MICROSOFT_SESSION_CACHE)
}

fn load_cached_microsoft_profile() -> Option<UserProfile> {
    let cache_path = microsoft_session_cache_path();
    let raw_profile = fs::read_to_string(cache_path).ok()?;
    serde_json::from_str::<UserProfile>(&raw_profile).ok()
}

fn save_cached_microsoft_profile(profile: &UserProfile) -> Result<(), String> {
    let cache_path = microsoft_session_cache_path();
    let cache_dir = cache_path
        .parent()
        .ok_or_else(|| "Session cache path has no parent directory.".to_string())?;
    fs::create_dir_all(cache_dir).map_err(|e| format!("Failed to prepare session cache: {}", e))?;

    let raw_profile = serde_json::to_string_pretty(profile)
        .map_err(|e| format!("Failed to serialize Microsoft session: {}", e))?;
    fs::write(cache_path, raw_profile).map_err(|e| format!("Failed to save session cache: {}", e))
}

fn clear_cached_microsoft_profile() {
    let _ = fs::remove_file(microsoft_session_cache_path());
}

fn launcher_config_path() -> PathBuf {
    LAUNCHER_DIR
        .config_dir()
        .to_path_buf()
        .join(LAUNCHER_CONFIG_FILE)
}

fn instance_dir() -> PathBuf {
    LAUNCHER_DIR
        .data_dir()
        .to_path_buf()
        .join("Fusion-Core-Instance")
}

fn clamp_mod_profile(profile: u8) -> u8 {
    profile.clamp(1, MAX_MOD_PROFILES)
}

fn default_mod_profile() -> u8 {
    1
}

fn mod_profile_name(profile: u8) -> String {
    format!("Profile {}", clamp_mod_profile(profile))
}

fn profile_root_dir(profile: u8) -> PathBuf {
    let profile = clamp_mod_profile(profile);
    if profile == 1 {
        instance_dir()
    } else {
        instance_dir()
            .join("mod-profiles")
            .join(format!("profile-{}", profile))
    }
}

fn profile_instances_dir(profile: u8) -> PathBuf {
    profile_root_dir(profile).join("instances")
}

fn instance_folder_name(version: &str, loader: &str) -> String {
    let sanitize = |value: &str| {
        value
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .trim_matches('-')
            .to_string()
    };

    format!("{}-{}", sanitize(version), sanitize(loader))
}

fn isolated_instance_dir(profile: u8, version: &str, loader: &str) -> PathBuf {
    profile_instances_dir(profile).join(instance_folder_name(version, loader))
}

fn isolated_instance_subdir(profile: u8, version: &str, loader: &str, subdir: &str) -> PathBuf {
    isolated_instance_dir(profile, version, loader).join(subdir)
}

fn isolated_instance_mods_dir(profile: u8, version: &str, loader: &str) -> PathBuf {
    isolated_instance_subdir(profile, version, loader, "mods")
}

fn isolated_instance_shaderpacks_dir(profile: u8, version: &str, loader: &str) -> PathBuf {
    isolated_instance_subdir(profile, version, loader, "shaderpacks")
}

fn legacy_profile_mods_dir(profile: u8) -> PathBuf {
    profile_root_dir(profile).join("mods")
}

fn legacy_nested_instance_dir(profile: u8, version: &str, loader: &str) -> PathBuf {
    legacy_profile_mods_dir(profile).join(instance_folder_name(version, loader))
}

fn legacy_nested_instance_mods_dir(profile: u8, version: &str, loader: &str) -> PathBuf {
    legacy_nested_instance_dir(profile, version, loader).join("mods")
}

fn mod_display_name(filename: &str) -> String {
    let base = filename
        .strip_suffix(".jar.bak")
        .or_else(|| filename.strip_suffix(".jar"))
        .unwrap_or(filename);
    let parts = base.split('-').collect::<Vec<_>>();
    let mut shortened = Vec::new();

    for part in parts {
        shortened.push(part);
        let clean_part = part.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.');
        let is_mc_version = (clean_part.starts_with("1.") || clean_part.starts_with("26."))
            && clean_part
                .chars()
                .all(|ch| ch.is_ascii_digit() || ch == '.');

        if is_mc_version {
            return shortened.join("-");
        }
    }

    base.to_string()
}

fn normalized_mod_name(filename: &str) -> String {
    mod_display_name(filename)
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn minecraft_release_tuple(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u32>().ok()?;

    Some((major, minor, patch))
}

fn minecraft_version_at_least(version: &str, major: u32, minor: u32, patch: u32) -> bool {
    minecraft_release_tuple(version).map_or(false, |current| current >= (major, minor, patch))
}

fn fallback_loader_supported_for_version(loader: &str, version: &str) -> bool {
    match loader {
        "Vanilla" => true,
        "Fabric" => version.starts_with("1.") && minecraft_version_at_least(version, 1, 14, 0),
        "Quilt" => version.starts_with("1.") && minecraft_version_at_least(version, 1, 18, 2),
        "NeoForge" => {
            version.starts_with("26.")
                || (version.starts_with("1.") && minecraft_version_at_least(version, 1, 20, 1))
        }
        "Forge" => version == "1.20.1",
        _ => false,
    }
}

fn system_cpu_limit() -> u32 {
    std::thread::available_parallelism()
        .map(|cores| cores.get() as u32)
        .unwrap_or(1)
        .max(1)
}

#[cfg(windows)]
fn system_ram_limit_gb() -> u32 {
    #[repr(C)]
    struct MemoryStatusEx {
        dw_length: u32,
        dw_memory_load: u32,
        ull_total_phys: u64,
        ull_avail_phys: u64,
        ull_total_page_file: u64,
        ull_avail_page_file: u64,
        ull_total_virtual: u64,
        ull_avail_virtual: u64,
        ull_avail_extended_virtual: u64,
    }

    extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    let mut memory_status = MemoryStatusEx {
        dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
        dw_memory_load: 0,
        ull_total_phys: 0,
        ull_avail_phys: 0,
        ull_total_page_file: 0,
        ull_avail_page_file: 0,
        ull_total_virtual: 0,
        ull_avail_virtual: 0,
        ull_avail_extended_virtual: 0,
    };

    let ok = unsafe { GlobalMemoryStatusEx(&mut memory_status) };
    if ok == 0 || memory_status.ull_total_phys == 0 {
        return 32;
    }

    let gib = 1024_u64 * 1024 * 1024;
    ((memory_status.ull_total_phys + gib - 1) / gib).clamp(1, u32::MAX as u64) as u32
}

#[cfg(not(windows))]
fn system_ram_limit_gb() -> u32 {
    32
}

fn load_launcher_config() -> LauncherConfig {
    let config_path = launcher_config_path();
    let raw_config = match fs::read_to_string(config_path) {
        Ok(raw) => raw,
        Err(_) => return LauncherConfig::default(),
    };

    serde_json::from_str(&raw_config).unwrap_or_default()
}

fn save_launcher_config(config: &LauncherConfig) -> Result<(), String> {
    let config_path = launcher_config_path();
    let config_dir = config_path
        .parent()
        .ok_or_else(|| "Launcher config path has no parent directory.".to_string())?;
    fs::create_dir_all(config_dir)
        .map_err(|e| format!("Failed to prepare config folder: {}", e))?;

    let raw_config = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize launcher config: {}", e))?;
    fs::write(config_path, raw_config).map_err(|e| format!("Failed to save launcher config: {}", e))
}

fn load_mod_manifest(mods_dir: &PathBuf) -> HashMap<String, InstalledModMetadata> {
    let manifest_path = mods_dir.join(MOD_MANIFEST_FILE);
    let raw_manifest = match fs::read_to_string(manifest_path) {
        Ok(raw) => raw,
        Err(_) => return HashMap::new(),
    };

    serde_json::from_str(&raw_manifest).unwrap_or_default()
}

fn save_mod_manifest(
    mods_dir: &PathBuf,
    manifest: &HashMap<String, InstalledModMetadata>,
) -> Result<(), String> {
    let raw_manifest = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Failed to serialize mod manifest: {}", e))?;
    fs::write(mods_dir.join(MOD_MANIFEST_FILE), raw_manifest)
        .map_err(|e| format!("Failed to save mod manifest: {}", e))
}

fn load_shader_manifest(shaderpacks_dir: &PathBuf) -> HashMap<String, String> {
    let manifest_path = shaderpacks_dir.join(SHADER_MANIFEST_FILE);
    let raw_manifest = match fs::read_to_string(manifest_path) {
        Ok(raw) => raw,
        Err(_) => return HashMap::new(),
    };

    serde_json::from_str(&raw_manifest).unwrap_or_default()
}

fn save_shader_manifest(
    shaderpacks_dir: &PathBuf,
    manifest: &HashMap<String, String>,
) -> Result<(), String> {
    let raw_manifest = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Failed to serialize shader manifest: {}", e))?;
    fs::write(shaderpacks_dir.join(SHADER_MANIFEST_FILE), raw_manifest)
        .map_err(|e| format!("Failed to save shader manifest: {}", e))
}

fn prepare_isolated_instance(instance_dir: &PathBuf) -> Result<(), String> {
    for subdir in [
        "mods",
        "config",
        "saves",
        "resourcepacks",
        "shaderpacks",
        "logs",
    ] {
        fs::create_dir_all(instance_dir.join(subdir))
            .map_err(|e| format!("Failed to prepare {}: {}", subdir, e))?;
    }

    Ok(())
}

fn find_executable_on_path(executable_name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;

    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(executable_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn mod_metadata_matches_environment(
    metadata: &InstalledModMetadata,
    selected_version: &str,
    selected_loader: &str,
) -> bool {
    let version_matches = if metadata.game_versions.is_empty() {
        metadata.game_version == selected_version
    } else {
        metadata
            .game_versions
            .iter()
            .any(|version| version == selected_version)
    };

    let selected_loader = selected_loader.to_ascii_lowercase();
    let loader_matches = if metadata.loaders.is_empty() {
        let loader = metadata.loader.to_ascii_lowercase();
        loader == selected_loader
    } else {
        metadata.loaders.iter().any(|loader| {
            let loader = loader.to_ascii_lowercase();
            loader == selected_loader
        })
    };

    version_matches && loader_matches
}

fn modrinth_version_matches_profile(
    version: &ModrinthVersion,
    selected_version: &str,
    selected_loader: &str,
) -> bool {
    let version_matches = version
        .game_versions
        .iter()
        .any(|version| version == selected_version);

    let loader_matches = modrinth_loader_category(selected_loader).map_or(true, |loader| {
        version.loaders.iter().any(|candidate| candidate == loader)
    });

    version_matches && loader_matches
}

fn is_project_installed_for_profile(
    project_id: &str,
    slug: &str,
    title: &str,
    selected_version: &str,
    selected_loader: &str,
    selected_mod_profile: u8,
) -> bool {
    let mods_dir =
        isolated_instance_mods_dir(selected_mod_profile, selected_version, selected_loader);
    let manifest = load_mod_manifest(&mods_dir);

    let normalize = |value: &str| {
        value
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase()
    };
    let normalized_title = normalize(title);
    let normalized_slug = normalize(slug);

    manifest.iter().any(|(filename, metadata)| {
        let normalized_project = normalize(&metadata.project_id);
        (metadata.project_id == project_id
            || normalized_project == normalized_slug
            || normalized_project == normalized_title)
            && (mods_dir.join(filename).exists()
                || mods_dir.join(format!("{}.bak", filename)).exists())
    })
}

fn is_manifest_project_installed_for_profile(
    project_id: &str,
    slug: &str,
    title: &str,
    selected_version: &str,
    selected_loader: &str,
    selected_mod_profile: u8,
) -> bool {
    let mods_dir =
        isolated_instance_mods_dir(selected_mod_profile, selected_version, selected_loader);
    let manifest = load_mod_manifest(&mods_dir);

    let normalize = |value: &str| {
        value
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase()
    };
    let normalized_title = normalize(title);
    let normalized_slug = normalize(slug);

    manifest.iter().any(|(filename, metadata)| {
        let normalized_project = normalize(&metadata.project_id);
        (metadata.project_id == project_id
            || normalized_project == normalized_slug
            || normalized_project == normalized_title)
            && (mods_dir.join(filename).exists()
                || mods_dir.join(format!("{}.bak", filename)).exists())
    })
}

fn is_shader_installed_for_profile(
    project_id: &str,
    _slug: &str,
    _title: &str,
    selected_version: &str,
    selected_loader: &str,
    selected_mod_profile: u8,
) -> bool {
    let shaderpacks_dir =
        isolated_instance_shaderpacks_dir(selected_mod_profile, selected_version, selected_loader);
    let manifest = load_shader_manifest(&shaderpacks_dir);

    manifest
        .values()
        .any(|stored_project_id| stored_project_id == project_id)
}

fn loader_from_name(loader: &str) -> Result<Loader, String> {
    match loader {
        "Vanilla" => Ok(Loader::Vanilla),
        "Fabric" => Ok(Loader::Fabric),
        "Quilt" => Ok(Loader::Quilt),
        "NeoForge" => Ok(Loader::NeoForge),
        "Forge" => Ok(Loader::NeoForge),
        other => Err(format!("Unknown loader: {}", other)),
    }
}

async fn resolve_loader_version(loader: &str, minecraft_version: &str) -> Result<String, String> {
    match loader {
        "Vanilla" => Ok("vanilla".to_string()),
        "Fabric" => {
            #[derive(Deserialize)]
            struct FabricLoaderEntry {
                loader: FabricLoaderVersion,
            }

            #[derive(Deserialize)]
            struct FabricLoaderVersion {
                version: String,
            }

            let url = format!(
                "https://meta.fabricmc.net/v2/versions/loader/{}",
                minecraft_version
            );
            let versions = reqwest::get(url)
                .await
                .map_err(|e| format!("Failed to fetch Fabric loader versions: {}", e))?
                .error_for_status()
                .map_err(|e| format!("Fabric loader lookup failed: {}", e))?
                .json::<Vec<FabricLoaderEntry>>()
                .await
                .map_err(|e| format!("Failed to read Fabric loader versions: {}", e))?;

            versions
                .first()
                .map(|entry| entry.loader.version.clone())
                .ok_or_else(|| format!("No Fabric loader found for {}.", minecraft_version))
        }
        "Quilt" => {
            #[derive(Deserialize)]
            struct QuiltLoaderEntry {
                loader: QuiltLoaderVersion,
            }

            #[derive(Deserialize)]
            struct QuiltLoaderVersion {
                version: String,
            }

            let url = format!(
                "https://meta.quiltmc.org/v3/versions/loader/{}",
                minecraft_version
            );
            let versions = reqwest::get(url)
                .await
                .map_err(|e| format!("Failed to fetch Quilt loader versions: {}", e))?
                .error_for_status()
                .map_err(|e| format!("Quilt loader lookup failed: {}", e))?
                .json::<Vec<QuiltLoaderEntry>>()
                .await
                .map_err(|e| format!("Failed to read Quilt loader versions: {}", e))?;

            versions
                .first()
                .map(|entry| entry.loader.version.clone())
                .ok_or_else(|| format!("No Quilt loader found for {}.", minecraft_version))
        }
        "NeoForge" => resolve_neoforge_version(minecraft_version).await,
        "Forge" => resolve_neoforged_forge_version(minecraft_version).await,
        other => Err(format!("Unknown loader: {}", other)),
    }
}

fn forge_version_key(full_version: &str, minecraft_version: &str) -> Vec<u32> {
    full_version
        .strip_prefix(&format!("{}-", minecraft_version))
        .unwrap_or(full_version)
        .split(|ch: char| !ch.is_ascii_digit())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

async fn resolve_neoforged_forge_version(minecraft_version: &str) -> Result<String, String> {
    let metadata =
        reqwest::get("https://maven.neoforged.net/releases/net/neoforged/forge/maven-metadata.xml")
            .await
            .map_err(|e| format!("Failed to fetch Forge metadata: {}", e))?
            .error_for_status()
            .map_err(|e| format!("Forge metadata lookup failed: {}", e))?
            .text()
            .await
            .map_err(|e| format!("Failed to read Forge metadata: {}", e))?;

    let prefix = format!("{}-", minecraft_version);
    let mut matches = metadata
        .split("<version>")
        .filter_map(|chunk| chunk.split("</version>").next())
        .filter(|version| version.starts_with(&prefix))
        .map(str::to_string)
        .collect::<Vec<_>>();

    matches.sort_by_key(|version| forge_version_key(version, minecraft_version));
    matches
        .pop()
        .and_then(|version| version.strip_prefix(&prefix).map(str::to_string))
        .ok_or_else(|| format!("No Forge loader found for {}.", minecraft_version))
}

async fn resolve_neoforge_version(minecraft_version: &str) -> Result<String, String> {
    let metadata = reqwest::get(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml",
    )
    .await
    .map_err(|e| format!("Failed to fetch NeoForge metadata: {}", e))?
    .error_for_status()
    .map_err(|e| format!("NeoForge metadata lookup failed: {}", e))?
    .text()
    .await
    .map_err(|e| format!("Failed to read NeoForge metadata: {}", e))?;

    let prefix = if let Some(rest) = minecraft_version.strip_prefix("1.") {
        let mut parts = rest.split('.');
        let Some(minor) = parts.next() else {
            return Err(format!(
                "Cannot map {} to a NeoForge version.",
                minecraft_version
            ));
        };
        let patch = parts.next().unwrap_or("0");
        format!("{}.{}.", minor, patch)
    } else if let Some(rest) = minecraft_version.strip_prefix("26.") {
        format!("26.{}.", rest.split('.').next().unwrap_or("1"))
    } else {
        return Err(format!(
            "Cannot map {} to a NeoForge version.",
            minecraft_version
        ));
    };

    let mut matches = metadata
        .split("<version>")
        .filter_map(|chunk| chunk.split("</version>").next())
        .filter(|version| version.starts_with(&prefix))
        .map(str::to_string)
        .collect::<Vec<_>>();

    matches.sort();
    matches
        .pop()
        .ok_or_else(|| format!("No NeoForge loader found for {}.", minecraft_version))
}

fn parse_java_major(version_output: &str) -> Option<u8> {
    let quoted = version_output.split('"').nth(1)?;
    let mut parts = quoted.split('.');
    let first = parts.next()?;

    if first == "1" {
        parts.next()?.parse().ok()
    } else {
        first.parse().ok()
    }
}

fn installed_java_major(java_path: &PathBuf) -> Option<u8> {
    let output = Command::new(java_path)
        .arg("-version")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let mut version_output = String::from_utf8_lossy(&output.stderr).to_string();
    version_output.push_str(&String::from_utf8_lossy(&output.stdout));
    parse_java_major(&version_output)
}

fn system_java_is_compatible(system_major: Option<u8>, required_major: u8) -> bool {
    match system_major {
        Some(major) if required_major <= 8 => major == required_major,
        Some(major) => major >= required_major,
        None => true,
    }
}

async fn select_java_binary(
    version: &impl VersionInfo,
    version_data: &Version,
    terminal_logs: &Arc<Mutex<Vec<String>>>,
) -> Result<PathBuf, String> {
    let required_major = version_data.java_version.major_version;
    let distribution = JavaDistribution::Temurin;

    if let Some(system_java) = FusionLauncherApp::system_java_path() {
        let system_major = installed_java_major(&system_java);
        if system_java_is_compatible(system_major, required_major) {
            let version_label = system_major
                .map(|major| major.to_string())
                .unwrap_or_else(|| "unknown version".to_string());
            push_terminal_log(
                terminal_logs,
                &format!(
                    "Using system Java {} at {}.",
                    version_label,
                    system_java.display()
                ),
            );
            return Ok(system_java);
        }

        push_terminal_log(
            terminal_logs,
            &format!(
                "System Java at {} is version {:?}, but Minecraft requires Java {}.",
                system_java.display(),
                system_major,
                required_major
            ),
        );
    }

    match find_java_binary(version.java_dirs(), &distribution, &required_major).await {
        Ok(path) => {
            push_terminal_log(
                terminal_logs,
                &format!(
                    "Using launcher-managed Java {} at {}.",
                    required_major,
                    path.display()
                ),
            );
            return Ok(path);
        }
        Err(e) => {
            push_terminal_log(
                terminal_logs,
                &format!(
                    "Launcher-managed Java {} was not ready: {}.",
                    required_major, e
                ),
            );
        }
    }

    push_terminal_log(
        terminal_logs,
        &format!(
            "Downloading launcher-managed Temurin Java {} because no compatible Java was found.",
            required_major
        ),
    );

    jre_download(
        version.java_dirs(),
        &distribution,
        &required_major,
        |_current, _total| {},
        None,
    )
    .await
    .map_err(|e| format!("JRE download failed: {}", e))
}

fn append_raw_log(path: &PathBuf, text: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(text.as_bytes());
    }
}

fn append_crash_log_raw(text: &str) {
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(crash_log_path())
    {
        let _ = file.write_all(text.as_bytes());
    }
}

fn append_minecraft_output(stream_name: &str, bytes: &[u8], process_log_path: &PathBuf) {
    let text = String::from_utf8_lossy(bytes);
    let entry = format!("[Minecraft {stream_name}] {text}");
    append_raw_log(process_log_path, &entry);

    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    append_crash_log_raw(&format!("[{timestamp}] {entry}"));
}

async fn pipe_minecraft_output<R>(
    mut reader: R,
    stream_name: &'static str,
    process_log_path: PathBuf,
) where
    R: AsyncRead + Unpin,
{
    let mut buffer = [0_u8; 8192];

    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => append_minecraft_output(stream_name, &buffer[..read], &process_log_path),
            Err(e) => {
                append_raw_log(
                    &process_log_path,
                    &format!("\n[Launcher] Failed to read Minecraft {stream_name}: {e}\n"),
                );
                break;
            }
        }
    }
}

fn latest_log_was_touched(game_dir: &PathBuf, launch_started: SystemTime) -> bool {
    let latest_log = game_dir.join("logs").join("latest.log");
    let threshold = launch_started
        .checked_sub(Duration::from_secs(5))
        .unwrap_or(UNIX_EPOCH);

    latest_log
        .metadata()
        .and_then(|metadata| metadata.modified())
        .map(|modified| modified >= threshold)
        .unwrap_or(false)
}

async fn execute_minecraft_process(
    java_path: PathBuf,
    arguments: Vec<String>,
    game_dir: PathBuf,
    launch_started: SystemTime,
    terminal_logs: Arc<Mutex<Vec<String>>>,
) -> Result<(), String> {
    let process_log_path = game_dir.join("logs").join("launcher-process.log");
    append_raw_log(
        &process_log_path,
        &format!(
            "\n=== Minecraft launch {} ===\nJava: {}\nWorking directory: {}\nArguments:\n{}\n\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            java_path.display(),
            game_dir.display(),
            arguments.join("\n")
        ),
    );
    push_terminal_log(
        &terminal_logs,
        &format!(
            "Minecraft process output is being written to {}.",
            process_log_path.display()
        ),
    );

    let mut child = TokioCommand::new(&java_path)
        .current_dir(&game_dir)
        .args(&arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start Java at {}: {}", java_path.display(), e))?;

    if let Some(pid) = child.id() {
        push_terminal_log(
            &terminal_logs,
            &format!("Minecraft Java process started as PID {pid}."),
        );
    }

    let stdout_task = child.stdout.take().map(|stdout| {
        tokio::spawn(pipe_minecraft_output(
            stdout,
            "stdout",
            process_log_path.clone(),
        ))
    });
    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(pipe_minecraft_output(
            stderr,
            "stderr",
            process_log_path.clone(),
        ))
    });

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed while waiting for Minecraft: {}", e))?;

    if let Some(task) = stdout_task {
        let _ = task.await;
    }
    if let Some(task) = stderr_task {
        let _ = task.await;
    }

    append_raw_log(
        &process_log_path,
        &format!("\n[Launcher] Minecraft process exited with status {status}.\n"),
    );

    let exit_code = status.code().unwrap_or(7900);
    if !status.success() && exit_code != -1073740791 {
        return Err(format!(
            "Minecraft exited with code {}. See crash.log and {}.",
            exit_code,
            process_log_path.display()
        ));
    }

    if !latest_log_was_touched(&game_dir, launch_started) {
        return Err(format!(
            "Minecraft exited without creating a fresh latest.log. See crash.log and {}.",
            process_log_path.display()
        ));
    }

    Ok(())
}

fn ensure_game_dir_argument(mut arguments: Vec<String>, game_dir: &PathBuf) -> Vec<String> {
    let game_dir_value = game_dir.to_string_lossy().to_string();
    let mut found_game_dir = false;
    let mut index = 0;

    while index < arguments.len() {
        if arguments[index] == "--gameDir" {
            found_game_dir = true;
            if index + 1 < arguments.len() {
                arguments[index + 1] = game_dir_value.clone();
                index += 2;
                continue;
            }

            arguments.push(game_dir_value.clone());
            break;
        }

        index += 1;
    }

    if !found_game_dir {
        arguments.push("--gameDir".to_string());
        arguments.push(game_dir_value);
    }

    arguments
}

fn build_minecraft_launch_arguments<V: LaunchArguments>(
    version: &V,
    version_data: &Version,
    profile: &UserProfile,
    request: &LaunchRequest,
) -> Vec<String> {
    let mut jvm_overrides = HashMap::new();
    let jvm_removals = HashSet::new();
    let mut arg_overrides = HashMap::new();
    let arg_removals = HashSet::new();
    let raw_args = Vec::new();

    jvm_overrides.insert("Xmx".to_string(), format!("{}G", request.allocated_ram_gb));
    jvm_overrides.insert("Xms".to_string(), "1G".to_string());

    if request.enable_gpu_optimizations {
        jvm_overrides.insert("XX:+UseG1GC".to_string(), String::new());
        jvm_overrides.insert("XX:+UnlockExperimentalVMOptions".to_string(), String::new());
    }

    for arg in request.custom_jvm_args.split_whitespace() {
        let trimmed = arg.trim_start_matches('-');
        if trimmed.is_empty() || trimmed.starts_with("Xmx") || trimmed.starts_with("Xms") {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            jvm_overrides.insert(key.to_string(), value.to_string());
        } else {
            jvm_overrides.insert(trimmed.to_string(), String::new());
        }
    }

    arg_overrides.insert(
        KEY_GAME_DIRECTORY.to_string(),
        request.game_dir.to_string_lossy().to_string(),
    );
    arg_overrides.insert(KEY_LAUNCHER_NAME.to_string(), "FusionLauncher".to_string());
    arg_overrides.insert(
        KEY_LAUNCHER_VERSION.to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );

    ensure_game_dir_argument(
        version.build_arguments(
            version_data,
            &profile.username,
            &profile.uuid,
            &arg_overrides,
            &arg_removals,
            &jvm_overrides,
            &jvm_removals,
            &raw_args,
        ),
        &request.game_dir,
    )
}

async fn launch_minecraft(
    request: LaunchRequest,
    terminal_logs: Arc<Mutex<Vec<String>>>,
) -> Result<(), String> {
    let launch_started = SystemTime::now();
    push_terminal_log(
        &terminal_logs,
        &format!(
            "Launch requested for {} {}.",
            request.selected_loader, request.selected_version
        ),
    );
    if request.selected_loader == "Forge" {
        push_terminal_log(
            &terminal_logs,
            "Forge profile selected; using the NeoForge-compatible launch backend.",
        );
    }

    let loader = loader_from_name(&request.selected_loader)?;
    let loader_version =
        resolve_loader_version(&request.selected_loader, &request.selected_version).await?;
    push_terminal_log(
        &terminal_logs,
        &format!(
            "Resolved {} loader version: {}.",
            request.selected_loader, loader_version
        ),
    );

    let profile = match request.auth_mode {
        AuthMode::Microsoft => request.ms_profile.clone().ok_or_else(|| {
            "Microsoft mode is selected but no Microsoft session is active.".to_string()
        })?,
        AuthMode::Offline => {
            let username = request.username.trim();
            if username.is_empty() {
                return Err("Offline username cannot be empty.".to_string());
            }

            OfflineAuth::new(username)
                .authenticate(None)
                .await
                .map_err(|e| format!("Failed to create offline profile: {}", e))?
        }
    };
    push_terminal_log(
        &terminal_logs,
        &format!("Using player profile: {}.", profile.username),
    );

    let version = VersionBuilder::new(
        "Fusion-Core-Instance",
        loader,
        &loader_version,
        &request.selected_version,
        &LAUNCHER_DIR,
    )
    .with_custom_game_dir(request.game_dir.clone());
    prepare_isolated_instance(&request.game_dir)?;
    push_terminal_log(
        &terminal_logs,
        &format!(
            "Using {} isolated instance at {}.",
            mod_profile_name(request.selected_mod_profile),
            request.game_dir.display()
        ),
    );

    let metadata = match version.loader() {
        Loader::Vanilla => version.get_complete().await,
        Loader::Fabric => version.get_fabric_complete().await,
        Loader::Quilt => version.get_quilt_complete().await,
        Loader::NeoForge => version.get_neoforge_complete().await,
        Loader::LightyUpdater => version.get_lighty_updater_complete().await,
        other => return Err(format!("Unsupported loader: {:?}", other)),
    }
    .map_err(|e| format!("Metadata lookup failed: {:?}", e))?;

    let version_data = match metadata.as_ref() {
        VersionMetaData::Version(version) => version,
        _ => {
            return Err("Launch metadata did not include a runnable Minecraft version.".to_string())
        }
    };

    let java_path = select_java_binary(&version, version_data, &terminal_logs).await?;

    push_terminal_log(&terminal_logs, "Installing/verifying Minecraft files...");
    version
        .install(version_data, None)
        .await
        .map_err(|e| format!("Install failed: {:?}", e))?;

    let arguments = build_minecraft_launch_arguments(&version, version_data, &profile, &request);
    push_terminal_log(
        &terminal_logs,
        &format!("Launch --gameDir is {}.", request.game_dir.display()),
    );

    push_terminal_log(&terminal_logs, "Starting the Minecraft Java process...");

    execute_minecraft_process(
        java_path,
        arguments,
        request.game_dir.clone(),
        launch_started,
        Arc::clone(&terminal_logs),
    )
    .await?;

    push_terminal_log(&terminal_logs, "Minecraft process finished successfully.");
    Ok(())
}

async fn fetch_modrinth_project_version(
    client: &reqwest::Client,
    project_id: String,
    selected_version: &str,
    selected_loader: &str,
) -> Result<ModrinthVersion, String> {
    let game_versions = format!(r#"["{}"]"#, selected_version);
    let mut exact_request = client
        .get(format!(
            "https://api.modrinth.com/v2/project/{}/version",
            project_id
        ))
        .header("User-Agent", "FusionLauncher/0.1.0")
        .query(&[("game_versions", game_versions.as_str())]);

    if let Some(loader_category) = modrinth_loader_category(selected_loader) {
        let loaders = format!(r#"["{}"]"#, loader_category);
        exact_request = exact_request.query(&[("loaders", loaders.as_str())]);
    }

    let versions = exact_request
        .send()
        .await
        .map_err(|e| format!("Version lookup failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Version lookup was rejected: {}", e))?
        .json::<Vec<ModrinthVersion>>()
        .await
        .map_err(|e| format!("Version response could not be read: {}", e))?;

    if let Some(version) = versions.into_iter().next() {
        return Ok(version);
    }

    let mut fallback_request = client
        .get(format!(
            "https://api.modrinth.com/v2/project/{}/version",
            project_id
        ))
        .header("User-Agent", "FusionLauncher/0.1.0");

    if let Some(loader_category) = modrinth_loader_category(selected_loader) {
        let loaders = format!(r#"["{}"]"#, loader_category);
        fallback_request = fallback_request.query(&[("loaders", loaders.as_str())]);
    }

    let versions = fallback_request
        .send()
        .await
        .map_err(|e| format!("Version fallback lookup failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Version fallback lookup was rejected: {}", e))?
        .json::<Vec<ModrinthVersion>>()
        .await
        .map_err(|e| format!("Version fallback response could not be read: {}", e))?;

    versions
        .into_iter()
        .find(|version| {
            modrinth_version_matches_profile(version, selected_version, selected_loader)
        })
        .ok_or_else(|| "No compatible version was found for this profile.".to_string())
}

async fn fetch_modrinth_install_version(
    client: &reqwest::Client,
    target: ModrinthInstallTarget,
    selected_version: &str,
    selected_loader: &str,
) -> Result<ModrinthVersion, String> {
    match target {
        ModrinthInstallTarget::Version(version_id) => {
            let version = client
                .get(format!(
                    "https://api.modrinth.com/v2/version/{}",
                    version_id
                ))
                .header("User-Agent", "FusionLauncher/0.1.0")
                .send()
                .await
                .map_err(|e| format!("Version lookup failed: {}", e))?
                .error_for_status()
                .map_err(|e| format!("Version lookup was rejected: {}", e))?
                .json::<ModrinthVersion>()
                .await
                .map_err(|e| format!("Version response could not be read: {}", e))?;

            if modrinth_version_matches_profile(&version, selected_version, selected_loader) {
                Ok(version)
            } else {
                fetch_modrinth_project_version(
                    client,
                    version.project_id.clone(),
                    selected_version,
                    selected_loader,
                )
                .await
            }
        }
        ModrinthInstallTarget::Project(project_id) => {
            fetch_modrinth_project_version(client, project_id, selected_version, selected_loader)
                .await
        }
    }
}

fn modrinth_loader_category(loader: &str) -> Option<&'static str> {
    match loader {
        "Fabric" => Some("fabric"),
        "Quilt" => Some("quilt"),
        "Forge" => Some("forge"),
        "NeoForge" => Some("neoforge"),
        _ => None,
    }
}

async fn download_modrinth_version_file(
    client: &reqwest::Client,
    mods_dir: &PathBuf,
    version: &ModrinthVersion,
) -> Result<String, String> {
    let file = version
        .files
        .iter()
        .find(|file| file.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| "Compatible version did not include a downloadable file.".to_string())?;

    let bytes = client
        .get(&file.url)
        .header("User-Agent", "FusionLauncher/0.1.0")
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Download was rejected: {}", e))?
        .bytes()
        .await
        .map_err(|e| format!("Downloaded file could not be read: {}", e))?;

    let target_path = mods_dir.join(&file.filename);
    fs::write(&target_path, &bytes)
        .map_err(|e| format!("Failed to save {}: {}", file.filename, e))?;

    Ok(file.filename.clone())
}

async fn install_modrinth_project_with_deps(
    client: &reqwest::Client,
    mods_dir: &PathBuf,
    project_id: String,
    selected_version: &str,
    selected_loader: &str,
    mods_dirty: Arc<Mutex<bool>>,
) -> Result<Vec<String>, String> {
    let mut install_queue = VecDeque::from([ModrinthInstallTarget::Project(project_id)]);
    let mut seen_projects = HashSet::new();
    let mut seen_versions = HashSet::new();
    let mut installed_files = Vec::new();
    let mut manifest = load_mod_manifest(mods_dir);

    while let Some(target) = install_queue.pop_front() {
        match &target {
            ModrinthInstallTarget::Project(project_id) => {
                if !seen_projects.insert(project_id.clone()) {
                    continue;
                }
            }
            ModrinthInstallTarget::Version(version_id) => {
                if !seen_versions.insert(version_id.clone()) {
                    continue;
                }
            }
        }

        let version =
            fetch_modrinth_install_version(client, target, selected_version, selected_loader)
                .await?;

        seen_projects.insert(version.project_id.clone());
        seen_versions.insert(version.id.clone());

        let filename = download_modrinth_version_file(client, mods_dir, &version).await?;
        manifest.insert(
            filename.clone(),
            InstalledModMetadata {
                project_id: version.project_id.clone(),
                game_version: selected_version.to_string(),
                loader: selected_loader.to_string(),
                game_versions: if version.game_versions.is_empty() {
                    vec![selected_version.to_string()]
                } else {
                    version.game_versions.clone()
                },
                loaders: if version.loaders.is_empty() {
                    modrinth_loader_category(selected_loader)
                        .map(|loader| vec![loader.to_string()])
                        .unwrap_or_else(|| vec![selected_loader.to_ascii_lowercase()])
                } else {
                    version.loaders.clone()
                },
            },
        );
        installed_files.push(filename);
        *mods_dirty.lock().unwrap() = true;

        for dependency in version
            .dependencies
            .iter()
            .filter(|dependency| dependency.dependency_type == "required")
        {
            if let Some(project_id) = &dependency.project_id {
                if !seen_projects.contains(project_id) {
                    install_queue.push_back(ModrinthInstallTarget::Project(project_id.clone()));
                }
            } else if let Some(version_id) = &dependency.version_id {
                if !seen_versions.contains(version_id) {
                    install_queue.push_back(ModrinthInstallTarget::Version(version_id.clone()));
                }
            }
        }
    }

    save_mod_manifest(mods_dir, &manifest)?;
    Ok(installed_files)
}

async fn fetch_modrinth_shader_version(
    client: &reqwest::Client,
    project_id: String,
    selected_version: &str,
) -> Result<ModrinthVersion, String> {
    let game_versions = format!(r#"["{}"]"#, selected_version);
    let loaders = r#"["iris"]"#.to_string();

    let versions = client
        .get(format!(
            "https://api.modrinth.com/v2/project/{}/version",
            project_id
        ))
        .header("User-Agent", "FusionLauncher/0.1.0")
        .query(&[
            ("game_versions", game_versions.as_str()),
            ("loaders", loaders.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("Shader version lookup failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Shader version lookup was rejected: {}", e))?
        .json::<Vec<ModrinthVersion>>()
        .await
        .map_err(|e| format!("Shader version response could not be read: {}", e))?;

    versions.into_iter().next().ok_or_else(|| {
        "No Iris-compatible shader version was found for this Minecraft version.".to_string()
    })
}

async fn install_modrinth_shader_project(
    client: &reqwest::Client,
    shaderpacks_dir: &PathBuf,
    project_id: String,
    selected_version: &str,
) -> Result<String, String> {
    let version = fetch_modrinth_shader_version(client, project_id, selected_version).await?;
    let filename = download_modrinth_version_file(client, shaderpacks_dir, &version).await?;
    let mut manifest = load_shader_manifest(shaderpacks_dir);
    manifest.insert(filename.clone(), version.project_id.clone());
    save_shader_manifest(shaderpacks_dir, &manifest)?;

    Ok(filename)
}

#[derive(Deserialize, Clone, Debug)]
struct ModrinthResult {
    title: String,
    description: String,
    project_id: String,
    slug: String,
}

#[derive(Deserialize)]
struct ModrinthSearchResponse {
    hits: Vec<ModrinthResult>,
}

#[derive(Clone, Debug, Deserialize)]
struct ModrinthVersion {
    id: String,
    project_id: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    files: Vec<ModrinthVersionFile>,
    dependencies: Vec<ModrinthDependency>,
}

#[derive(Clone, Debug, Deserialize)]
struct ModrinthVersionFile {
    url: String,
    filename: String,
    primary: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct ModrinthDependency {
    project_id: Option<String>,
    version_id: Option<String>,
    dependency_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct InstalledModMetadata {
    #[serde(default)]
    project_id: String,
    #[serde(default)]
    game_version: String,
    #[serde(default)]
    loader: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct LauncherConfig {
    selected_version: String,
    selected_loader: String,
    #[serde(default = "default_mod_profile")]
    selected_mod_profile: u8,
    username: String,
    allocated_ram_gb: u32,
    cpu_cores: u32,
    use_dedicated_gpu: bool,
    enable_gpu_optimizations: bool,
    custom_jvm_args: String,
    skin_path_input: String,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            selected_version: "1.21.1".to_string(),
            selected_loader: "Fabric".to_string(),
            selected_mod_profile: 1,
            username: "Test".to_string(),
            allocated_ram_gb: 4,
            cpu_cores: 4,
            use_dedicated_gpu: true,
            enable_gpu_optimizations: true,
            custom_jvm_args: "-XX:+UseG1GC -XX:+UnlockExperimentalVMOptions".to_string(),
            skin_path_input: "".to_string(),
        }
    }
}

#[derive(Clone)]
struct LaunchRequest {
    username: String,
    selected_version: String,
    selected_loader: String,
    selected_mod_profile: u8,
    game_dir: PathBuf,
    auth_mode: AuthMode,
    ms_profile: Option<UserProfile>,
    allocated_ram_gb: u32,
    custom_jvm_args: String,
    enable_gpu_optimizations: bool,
}

enum ModrinthInstallTarget {
    Project(String),
    Version(String),
}

#[derive(Clone, PartialEq)]
enum ActiveTab {
    Play,
    Mods,
    Skins,
    Shaders,
    Settings,
}

#[derive(Clone, PartialEq, Debug)]
enum AuthMode {
    Offline,
    Microsoft,
}

// 🌐 Custom Microsoft Authenticator using your registered Fusion MC Client ID
struct CustomMicrosoftAuth {
    client_id: String,
}

impl CustomMicrosoftAuth {
    fn new(client_id: &str) -> Self {
        Self {
            client_id: client_id.to_string(),
        }
    }

    async fn authenticate_device(
        &self,
        status_text: Arc<Mutex<String>>,
        terminal_logs: Arc<Mutex<Vec<String>>>,
        ctx: egui::Context,
        cancel_token: Arc<Mutex<bool>>,
        refresh_trigger: Arc<Mutex<bool>>,
    ) -> Result<UserProfile, String> {
        let client = reqwest::Client::new();

        // 1. Request device code using the /consumers/ endpoint to bypass organization locks
        let device_url = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";

        let params = [
            ("client_id", self.client_id.as_str()),
            ("scope", "XboxLive.signin offline_access"),
        ];

        #[derive(Deserialize)]
        struct DeviceResponse {
            device_code: String,
            user_code: String,
            verification_uri: String,
            interval: u64,
        }

        let res = client
            .post(device_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Failed to initiate device flow: {}", e))?;

        let status_code = res.status();
        let raw_text = res
            .text()
            .await
            .map_err(|e| format!("Failed reading endpoint stream: {}", e))?;

        if !status_code.is_success() {
            return Err(format!(
                "Microsoft Endpoint Rejected Request ({}): {}",
                status_code, raw_text
            ));
        }

        let device_data: DeviceResponse = serde_json::from_str(&raw_text).map_err(|_e| {
            format!(
                "Structural format mismatch on device code payload. Raw: {}",
                raw_text
            )
        })?;

        // 📢 Direct UI and Log updates
        if let Ok(mut status) = status_text.lock() {
            *status = format!(
                "Go to {} and enter code: {}",
                device_data.verification_uri, device_data.user_code
            );
        }
        if let Ok(mut logs) = terminal_logs.lock() {
            logs.push(format!(
                "[AUTH] Action required: Visit {} with code {}",
                device_data.verification_uri, device_data.user_code
            ));
        }
        ctx.request_repaint();

        // 2. Poll for the Access Token
        let token_url = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
        let token_params = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", self.client_id.as_str()),
            ("device_code", device_data.device_code.as_str()),
        ];

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: Option<String>,
            error: Option<String>,
        }

        let interval = std::time::Duration::from_secs(device_data.interval.max(5));
        let access_token;

        loop {
            tokio::time::sleep(interval).await;

            // Check if user requested a code refresh or closed out
            if *cancel_token.lock().unwrap() || *refresh_trigger.lock().unwrap() {
                return Err("Authentication loop interrupted by user operation.".to_string());
            }

            let token_res = client
                .post(token_url)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .form(&token_params)
                .send()
                .await
                .map_err(|e| format!("Token request failure: {}", e))?;

            let token_data: TokenResponse = token_res
                .json()
                .await
                .map_err(|e| format!("Failed parsing token response: {}", e))?;

            if let Some(err) = token_data.error {
                if err == "authorization_pending" {
                    continue;
                } else {
                    return Err(format!("Microsoft authentication denied: {}", err));
                }
            }

            if let Some(tok) = token_data.access_token {
                access_token = tok;
                break;
            } else {
                return Err("Failed resolving access token from endpoint data stream.".to_string());
            }
        }

        if let Ok(mut logs) = terminal_logs.lock() {
            logs.push("[AUTH] Device token authorized. Handshaking with Xbox Live...".to_string());
        }
        ctx.request_repaint();

        // 3. Authenticate with Xbox Live
        let xbl_url = "https://user.auth.xboxlive.com/user/authenticate";

        #[allow(non_snake_case)]
        #[derive(Serialize)]
        struct XblProperties {
            AuthMethod: String,
            SiteName: String,
            RpsTicket: String,
        }

        #[allow(non_snake_case)]
        #[derive(Serialize)]
        struct XblPayload {
            Properties: XblProperties,
            RelyingParty: String,
            TokenType: String,
        }

        let xbl_payload = XblPayload {
            Properties: XblProperties {
                AuthMethod: "RPS".to_string(),
                SiteName: "user.auth.xboxlive.com".to_string(),
                RpsTicket: format!("d={}", access_token),
            },
            RelyingParty: "http://auth.xboxlive.com".to_string(),
            TokenType: "JWT".to_string(),
        };

        #[allow(non_snake_case)]
        #[derive(Deserialize)]
        struct XblDisplayClaims {
            xui: Vec<XblUhs>,
        }
        #[derive(Deserialize)]
        struct XblUhs {
            uhs: String,
        }

        #[allow(non_snake_case)]
        #[derive(Deserialize)]
        struct XblResponse {
            Token: String,
            DisplayClaims: XblDisplayClaims,
        }

        let xbl_res = client
            .post(xbl_url)
            .json(&xbl_payload)
            .send()
            .await
            .map_err(|e| format!("Xbox Live connection failed: {}", e))?;
        let xbl_data: XblResponse = xbl_res
            .json()
            .await
            .map_err(|e| format!("Invalid Xbox Live token response: {}", e))?;
        let uhs = xbl_data
            .DisplayClaims
            .xui
            .first()
            .ok_or("Missing user hashes.")?
            .uhs
            .clone();

        // 4. Authenticate with XSTS
        let xsts_url = "https://xsts.auth.xboxlive.com/xsts/authorize";

        #[allow(non_snake_case)]
        #[derive(Serialize)]
        struct XstsProperties {
            SandboxId: String,
            UserTokens: Vec<String>,
        }

        #[allow(non_snake_case)]
        #[derive(Serialize)]
        struct XstsPayload {
            Properties: XstsProperties,
            RelyingParty: String,
            TokenType: String,
        }

        let xsts_payload = XstsPayload {
            Properties: XstsProperties {
                SandboxId: "RETAIL".to_string(),
                UserTokens: vec![xbl_data.Token],
            },
            RelyingParty: "rp://api.minecraftservices.com/".to_string(),
            TokenType: "JWT".to_string(),
        };

        #[allow(non_snake_case)]
        #[derive(Deserialize)]
        struct XstsResponse {
            Token: String,
        }
        let xsts_res = client
            .post(xsts_url)
            .json(&xsts_payload)
            .send()
            .await
            .map_err(|e| format!("XSTS negotiation failed: {}", e))?;
        let xsts_data: XstsResponse = xsts_res
            .json()
            .await
            .map_err(|e| format!("Invalid XSTS response payload structure: {}", e))?;

        // 5. Authenticate with Minecraft Services
        let mc_login_url = "https://api.minecraftservices.com/authentication/login_with_xbox";

        #[allow(non_snake_case)]
        #[derive(Serialize)]
        struct McLoginPayload {
            identityToken: String,
        }

        let mc_payload = McLoginPayload {
            identityToken: format!("XBL3.0 x={};{}", uhs, xsts_data.Token),
        };

        #[derive(Deserialize)]
        struct McLoginResponse {
            access_token: String,
        }
        let mc_res = client
            .post(mc_login_url)
            .json(&mc_payload)
            .send()
            .await
            .map_err(|e| format!("Minecraft token resolution failed: {}", e))?;
        let mc_data: McLoginResponse = mc_res
            .json()
            .await
            .map_err(|e| format!("Invalid profile session details returned: {}", e))?;

        // 6. Fetch Minecraft Game Profile Data
        let profile_url = "https://api.minecraftservices.com/minecraft/profile";
        #[derive(Deserialize)]
        struct McProfileResponse {
            name: String,
        }
        let profile_res = client
            .get(profile_url)
            .bearer_auth(&mc_data.access_token)
            .send()
            .await
            .map_err(|e| format!("Profile fetch error: {}", e))?;

        let profile_data: McProfileResponse = profile_res
            .json()
            .await
            .map_err(|e| format!("Failed mapping user game identity: {}", e))?;

        // 7. Leverage lighty_launcher's native generator method via OfflineAuth to safely map fields
        let mut base_auth = OfflineAuth::new(&profile_data.name);
        let mut final_profile = base_auth.authenticate(None).await.map_err(|e| {
            format!(
                "Failed constructing native session profile container: {}",
                e
            )
        })?;

        final_profile.access_token = Some(mc_data.access_token);

        Ok(final_profile)
    }
}

fn main() -> eframe::Result<()> {
    install_crash_logger();
    set_windows_app_user_model_id();

    if let Err(err) = run_launcher() {
        write_crash_log(&format!("Launcher exited with error: {err}"));
        return Err(err);
    }

    Ok(())
}

#[cfg(windows)]
fn set_windows_app_user_model_id() {
    extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(appid: *const u16) -> i32;
    }

    let app_id = "Fusion.FusionLauncher.App\0"
        .encode_utf16()
        .collect::<Vec<_>>();
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
    }
}

#[cfg(not(windows))]
fn set_windows_app_user_model_id() {}

fn run_launcher() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Fusion Launcher")
            .with_inner_size([500.0, 620.0])
            .with_resizable(false)
            .with_icon(load_app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "Fusion Launcher",
        native_options,
        Box::new(|cc| Box::new(FusionLauncherApp::new(cc))),
    )
}

fn crash_log_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CRASH_LOG_FILE)
}

fn install_crash_logger() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        write_crash_log(&format!("Panic: {panic_info}"));
        default_hook(panic_info);
    }));
}

fn write_crash_log(message: &str) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let entry = format!("[{timestamp}] {message}\n\n");

    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(crash_log_path())
    {
        let _ = file.write_all(entry.as_bytes());
    }
}

fn push_terminal_log(logs: &Arc<Mutex<Vec<String>>>, msg: &str) {
    write_crash_log(msg);
    if let Ok(mut logs) = logs.lock() {
        logs.push(format!(
            "[{}] {}",
            chrono::Local::now().format("%H:%M:%S"),
            msg
        ));
    }
}

fn load_app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(APP_LOGO_PNG)
        .expect("Failed to load Fusion Launcher icon PNG.")
}

#[derive(Clone)]
enum CompatibilityResultKind {
    Error,
    Warning,
    Success,
    Info,
}

#[derive(Clone)]
struct CompatibilityResult {
    kind: CompatibilityResultKind,
    message: String,
}

struct FusionLauncherApp {
    username: String,
    selected_version: String,
    selected_loader: String,
    selected_mod_profile: u8,
    status_text: Arc<Mutex<String>>,
    is_launching: Arc<Mutex<bool>>,
    found_mods: Vec<(String, bool)>,
    compatibility_results: Vec<CompatibilityResult>,
    current_tab: ActiveTab,
    search_query: String,
    search_results: Arc<Mutex<Vec<ModrinthResult>>>,
    is_searching: Arc<Mutex<bool>>,
    mods_dirty: Arc<Mutex<bool>>,
    shader_search_query: String,
    shader_search_results: Arc<Mutex<Vec<ModrinthResult>>>,
    is_shader_searching: Arc<Mutex<bool>>,
    last_folder_scan: Instant,
    skin_path_input: String,
    terminal_logs: Arc<Mutex<Vec<String>>>,
    auth_mode: AuthMode,
    ms_profile: Arc<Mutex<Option<UserProfile>>>,
    is_authenticating: Arc<Mutex<bool>>,
    auth_cancel_token: Arc<Mutex<bool>>,
    auth_refresh_trigger: Arc<Mutex<bool>>,
    allocated_ram_gb: u32,
    cpu_cores: u32,
    use_dedicated_gpu: bool,
    enable_gpu_optimizations: bool,
    custom_jvm_args: String,
    java_status: Arc<Mutex<String>>,
    is_installing_java: Arc<Mutex<bool>>,
    launch_started_at: Option<Instant>,
    last_saved_config: LauncherConfig,
    loader_availability: Arc<Mutex<HashMap<String, HashMap<String, bool>>>>,
    is_checking_loader_availability: Arc<Mutex<bool>>,
    last_loader_availability_request: String,
    taskbar_icon_applied: bool,
}

impl FusionLauncherApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(26, 27, 38);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(43, 44, 64);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(65, 67, 97);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(88, 91, 133);
        visuals.window_rounding = 8.0.into();
        cc.egui_ctx.set_visuals(visuals);

        let logs = Arc::new(Mutex::new(vec![
            "[SYSTEM] Launcher core initialized.".to_string()
        ]));
        let launcher_config = load_launcher_config();
        let cached_profile = load_cached_microsoft_profile();
        if let Some(profile) = &cached_profile {
            if let Ok(mut terminal_logs) = logs.lock() {
                terminal_logs.push(format!(
                    "[AUTH] Restored cached Microsoft session for {}.",
                    profile.username
                ));
            }
        }

        let max_ram_gb = system_ram_limit_gb();
        let max_cpu_cores = system_cpu_limit();

        let mut app = Self {
            username: cached_profile
                .as_ref()
                .map(|profile| profile.username.clone())
                .unwrap_or_else(|| launcher_config.username.clone()),
            selected_version: launcher_config.selected_version.clone(),
            selected_loader: launcher_config.selected_loader.clone(),
            selected_mod_profile: clamp_mod_profile(launcher_config.selected_mod_profile),
            status_text: Arc::new(Mutex::new(
                cached_profile
                    .as_ref()
                    .map(|profile| format!("Welcome back, {}!", profile.username))
                    .unwrap_or_else(|| "Ready to launch client.".to_string()),
            )),
            is_launching: Arc::new(Mutex::new(false)),
            found_mods: Vec::new(),
            compatibility_results: Vec::new(),
            current_tab: ActiveTab::Play,
            search_query: "".to_string(),
            search_results: Arc::new(Mutex::new(Vec::new())),
            is_searching: Arc::new(Mutex::new(false)),
            mods_dirty: Arc::new(Mutex::new(false)),
            shader_search_query: "".to_string(),
            shader_search_results: Arc::new(Mutex::new(Vec::new())),
            is_shader_searching: Arc::new(Mutex::new(false)),
            last_folder_scan: Instant::now() - Duration::from_secs(5),
            skin_path_input: launcher_config.skin_path_input.clone(),
            terminal_logs: logs,
            auth_mode: if cached_profile.is_some() {
                AuthMode::Microsoft
            } else {
                AuthMode::Offline
            },
            ms_profile: Arc::new(Mutex::new(cached_profile)),
            is_authenticating: Arc::new(Mutex::new(false)),
            auth_cancel_token: Arc::new(Mutex::new(false)),
            auth_refresh_trigger: Arc::new(Mutex::new(false)),
            allocated_ram_gb: launcher_config.allocated_ram_gb.clamp(1, max_ram_gb),
            cpu_cores: launcher_config.cpu_cores.clamp(1, max_cpu_cores),
            use_dedicated_gpu: launcher_config.use_dedicated_gpu,
            enable_gpu_optimizations: launcher_config.enable_gpu_optimizations,
            custom_jvm_args: launcher_config.custom_jvm_args.clone(),
            java_status: Arc::new(Mutex::new("Checking Java runtime...".to_string())),
            is_installing_java: Arc::new(Mutex::new(false)),
            launch_started_at: None,
            last_saved_config: launcher_config,
            loader_availability: Arc::new(Mutex::new(HashMap::new())),
            is_checking_loader_availability: Arc::new(Mutex::new(false)),
            last_loader_availability_request: String::new(),
            taskbar_icon_applied: false,
        };

        let _ = app.migrate_legacy_mods_for_current_environment();
        app.refresh_mods_list();
        app.ensure_loader_availability_check(&cc.egui_ctx);
        app.check_java_runtime(&cc.egui_ctx);
        app
    }

    fn log_to_terminal(&self, msg: &str) {
        write_crash_log(msg);
        if let Ok(mut logs) = self.terminal_logs.lock() {
            logs.push(format!(
                "[{}] {}",
                chrono::Local::now().format("%H:%M:%S"),
                msg
            ));
        }
    }

    fn current_config(&self) -> LauncherConfig {
        LauncherConfig {
            selected_version: self.selected_version.clone(),
            selected_loader: self.selected_loader.clone(),
            selected_mod_profile: self.selected_mod_profile,
            username: self.username.clone(),
            allocated_ram_gb: self.allocated_ram_gb,
            cpu_cores: self.cpu_cores,
            use_dedicated_gpu: self.use_dedicated_gpu,
            enable_gpu_optimizations: self.enable_gpu_optimizations,
            custom_jvm_args: self.custom_jvm_args.clone(),
            skin_path_input: self.skin_path_input.clone(),
        }
    }

    fn save_config_if_changed(&mut self) {
        let current_config = self.current_config();
        if current_config.selected_version == self.last_saved_config.selected_version
            && current_config.selected_loader == self.last_saved_config.selected_loader
            && current_config.selected_mod_profile == self.last_saved_config.selected_mod_profile
            && current_config.username == self.last_saved_config.username
            && current_config.allocated_ram_gb == self.last_saved_config.allocated_ram_gb
            && current_config.cpu_cores == self.last_saved_config.cpu_cores
            && current_config.use_dedicated_gpu == self.last_saved_config.use_dedicated_gpu
            && current_config.enable_gpu_optimizations
                == self.last_saved_config.enable_gpu_optimizations
            && current_config.custom_jvm_args == self.last_saved_config.custom_jvm_args
            && current_config.skin_path_input == self.last_saved_config.skin_path_input
        {
            return;
        }

        match save_launcher_config(&current_config) {
            Ok(()) => {
                self.last_saved_config = current_config;
            }
            Err(e) => {
                self.log_to_terminal(&format!("Failed to save launcher config: {}", e));
            }
        }
    }

    fn logout_microsoft(&mut self) {
        self.auth_mode = AuthMode::Offline;
        *self.ms_profile.lock().unwrap() = None;
        clear_cached_microsoft_profile();
        self.log_to_terminal(
            "Official Microsoft security context cleared. Reverted back to offline simulation rules.",
        );
    }

    fn launch_settings_summary(&self) -> String {
        let gpu_mode = if self.use_dedicated_gpu {
            "dedicated GPU preferred"
        } else {
            "system default GPU"
        };
        let gpu_optimizations = if self.enable_gpu_optimizations {
            "GPU optimizations on"
        } else {
            "GPU optimizations off"
        };

        format!(
            "{}GB RAM, {} CPU cores, {}, {}, JVM args: {}",
            self.allocated_ram_gb,
            self.cpu_cores,
            gpu_mode,
            gpu_optimizations,
            self.custom_jvm_args.trim()
        )
    }

    fn current_game_dir(&self) -> PathBuf {
        isolated_instance_dir(
            self.selected_mod_profile,
            &self.selected_version,
            &self.selected_loader,
        )
    }

    fn current_mods_dir(&self) -> PathBuf {
        isolated_instance_mods_dir(
            self.selected_mod_profile,
            &self.selected_version,
            &self.selected_loader,
        )
    }

    fn current_shaderpacks_dir(&self) -> PathBuf {
        isolated_instance_shaderpacks_dir(
            self.selected_mod_profile,
            &self.selected_version,
            &self.selected_loader,
        )
    }

    fn iris_supported_for_selected_loader(&self) -> bool {
        matches!(
            self.selected_loader.as_str(),
            "Fabric" | "Quilt" | "NeoForge"
        )
    }

    fn migrate_legacy_mods_for_current_environment(&mut self) -> Result<usize, String> {
        let legacy_mods_dir = legacy_profile_mods_dir(self.selected_mod_profile);
        let legacy_nested_mods_dir = legacy_nested_instance_mods_dir(
            self.selected_mod_profile,
            &self.selected_version,
            &self.selected_loader,
        );
        let mods_dir = self.current_mods_dir();
        prepare_isolated_instance(&self.current_game_dir())?;

        let mut migrated_count = 0;
        let mut target_manifest = load_mod_manifest(&mods_dir);

        let migrate_file = |source_dir: &PathBuf, filename: &str| -> Result<bool, String> {
            let source_path = source_dir.join(filename);
            if !source_path.exists() {
                return Ok(false);
            }

            let target_path = mods_dir.join(filename);
            if target_path.exists() {
                return Ok(false);
            }

            fs::rename(&source_path, &target_path).map_err(|e| {
                format!(
                    "Failed to move {} to {}: {}",
                    source_path.display(),
                    target_path.display(),
                    e
                )
            })?;

            Ok(true)
        };

        if legacy_nested_mods_dir.exists() && legacy_nested_mods_dir != mods_dir {
            let nested_manifest = load_mod_manifest(&legacy_nested_mods_dir);
            for entry in fs::read_dir(&legacy_nested_mods_dir)
                .map_err(|e| format!("Failed to scan legacy nested mods folder: {}", e))?
                .flatten()
            {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                let filename = entry.file_name().to_string_lossy().to_string();
                if filename == MOD_MANIFEST_FILE {
                    continue;
                }

                if migrate_file(&legacy_nested_mods_dir, &filename)? {
                    migrated_count += 1;
                }
            }

            for (filename, metadata) in nested_manifest {
                target_manifest.insert(filename, metadata);
            }
        }

        let legacy_manifest = load_mod_manifest(&legacy_mods_dir);
        for (filename, metadata) in legacy_manifest {
            if !mod_metadata_matches_environment(
                &metadata,
                &self.selected_version,
                &self.selected_loader,
            ) {
                continue;
            }

            if migrate_file(&legacy_mods_dir, &filename)? {
                migrated_count += 1;
            }

            let disabled_filename = format!("{}.bak", filename);
            if migrate_file(&legacy_mods_dir, &disabled_filename)? {
                migrated_count += 1;
            }

            target_manifest.insert(filename, metadata);
        }

        if migrated_count > 0 {
            save_mod_manifest(&mods_dir, &target_manifest)?;
            self.log_to_terminal(&format!(
                "Migrated {} legacy mod{} into {}.",
                migrated_count,
                if migrated_count == 1 { "" } else { "s" },
                mods_dir.display()
            ));
        }

        Ok(migrated_count)
    }

    fn prepare_current_mod_profile_for_launch(&mut self) -> Result<(), String> {
        prepare_isolated_instance(&self.current_game_dir())?;
        Ok(())
    }

    fn system_java_path() -> Option<PathBuf> {
        for executable in ["java.exe", "java"] {
            if let Some(java_path) = find_executable_on_path(executable) {
                let java_works = Command::new(&java_path)
                    .arg("-version")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false);

                if java_works {
                    return Some(java_path);
                }
            }
        }

        if let Ok(java_home) = env::var("JAVA_HOME") {
            let java_home_path = PathBuf::from(java_home).join("bin").join("java.exe");
            if java_home_path.exists() {
                return Some(java_home_path);
            }
        }

        let program_files =
            env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
        let candidates = [
            "Eclipse Adoptium",
            "Java",
            "Microsoft",
            "BellSoft",
            "Amazon Corretto",
        ];

        for vendor in candidates {
            let vendor_dir = PathBuf::from(&program_files).join(vendor);
            let Ok(entries) = fs::read_dir(vendor_dir) else {
                continue;
            };

            for entry in entries.flatten() {
                let java_path = entry.path().join("bin").join("java.exe");
                if java_path.exists() {
                    return Some(java_path);
                }
            }
        }

        None
    }

    fn check_java_runtime(&self, ctx: &egui::Context) {
        let java_status = Arc::clone(&self.java_status);
        let ctx_refresh = ctx.clone();

        std::thread::spawn(move || {
            let status = if let Some(path) = Self::system_java_path() {
                format!("Java runtime detected at {}.", path.display())
            } else {
                "System Java not found. Play can still use the launcher's managed Java runtime."
                    .to_string()
            };

            *java_status.lock().unwrap() = status;
            ctx_refresh.request_repaint();
        });
    }

    fn install_java_runtime(&self, ctx: &egui::Context) {
        if *self.is_installing_java.lock().unwrap() {
            return;
        }

        *self.is_installing_java.lock().unwrap() = true;
        *self.java_status.lock().unwrap() = "Installing Java 21 runtime...".to_string();
        *self.status_text.lock().unwrap() = "Installing Java runtime...".to_string();
        self.log_to_terminal(
            "Java was not detected. Starting Temurin Java 21 runtime install via winget...",
        );

        let java_status = Arc::clone(&self.java_status);
        let is_installing_java = Arc::clone(&self.is_installing_java);
        let status_text = Arc::clone(&self.status_text);
        let terminal_logs = Arc::clone(&self.terminal_logs);
        let ctx_refresh = ctx.clone();

        std::thread::spawn(move || {
            let install_result = Command::new("winget")
                .args([
                    "install",
                    "-e",
                    "--id",
                    "EclipseAdoptium.Temurin.21.JRE",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                    "--silent",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output();

            match install_result {
                Ok(output) if output.status.success() => {
                    *java_status.lock().unwrap() =
                        "Java install completed. Restart the launcher if Java is still not detected."
                            .to_string();
                    *status_text.lock().unwrap() = "Java runtime installed.".to_string();
                    if let Ok(mut logs) = terminal_logs.lock() {
                        logs.push("[JAVA] Temurin Java 21 runtime install completed.".to_string());
                    }
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let details = if stderr.trim().is_empty() {
                        stdout.trim()
                    } else {
                        stderr.trim()
                    };
                    *java_status.lock().unwrap() = "Java install failed.".to_string();
                    *status_text.lock().unwrap() = "Java install failed. Check logs.".to_string();
                    if let Ok(mut logs) = terminal_logs.lock() {
                        logs.push(format!(
                            "[ERROR] winget Java install failed with status {}: {}",
                            output.status, details
                        ));
                    }
                }
                Err(e) => {
                    *java_status.lock().unwrap() =
                        "Java install failed because winget could not run.".to_string();
                    *status_text.lock().unwrap() = "Java install failed. Check logs.".to_string();
                    if let Ok(mut logs) = terminal_logs.lock() {
                        logs.push(format!(
                            "[ERROR] Failed to start winget Java install: {}",
                            e
                        ));
                    }
                }
            }

            *is_installing_java.lock().unwrap() = false;
            ctx_refresh.request_repaint();
        });
    }

    fn ensure_java_before_launch(&self, ctx: &egui::Context) -> bool {
        if let Some(path) = Self::system_java_path() {
            *self.java_status.lock().unwrap() =
                format!("Java runtime detected at {}.", path.display());
        } else {
            *self.java_status.lock().unwrap() =
                "System Java not found. Using launcher-managed Java during launch.".to_string();
            self.log_to_terminal(
                "System Java was not found on PATH/JAVA_HOME. Continuing because the launch backend can install/use managed Java.",
            );
        }

        ctx.request_repaint();
        true
    }

    fn build_launch_request(&self) -> LaunchRequest {
        LaunchRequest {
            username: self.username.clone(),
            selected_version: self.selected_version.clone(),
            selected_loader: self.selected_loader.clone(),
            selected_mod_profile: self.selected_mod_profile,
            game_dir: self.current_game_dir(),
            auth_mode: self.auth_mode.clone(),
            ms_profile: self.ms_profile.lock().unwrap().clone(),
            allocated_ram_gb: self.allocated_ram_gb,
            custom_jvm_args: self.custom_jvm_args.clone(),
            enable_gpu_optimizations: self.enable_gpu_optimizations,
        }
    }

    fn trigger_game_launch(&mut self, ctx: &egui::Context) {
        if *self.is_launching.lock().unwrap() {
            return;
        }

        if !self.ensure_java_before_launch(ctx) {
            return;
        }

        if let Err(e) = self.prepare_current_mod_profile_for_launch() {
            *self.status_text.lock().unwrap() = e;
            return;
        }
        self.refresh_mods_list();

        self.launch_started_at = Some(Instant::now());
        let request = self.build_launch_request();
        let status_text = Arc::clone(&self.status_text);
        let is_launching = Arc::clone(&self.is_launching);
        let terminal_logs = Arc::clone(&self.terminal_logs);
        let ctx_refresh = ctx.clone();
        let launch_summary = self.launch_settings_summary();

        *is_launching.lock().unwrap() = true;
        *status_text.lock().unwrap() = format!(
            "Launching {} {}...",
            request.selected_loader, request.selected_version
        );
        self.log_to_terminal(&format!(
            "Assembling targeted environment variables for process injection execution: {}.",
            launch_summary
        ));

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let launch_result = launch_minecraft(request, Arc::clone(&terminal_logs)).await;

                match launch_result {
                    Ok(()) => {
                        *status_text.lock().unwrap() = "Minecraft closed.".to_string();
                        if let Ok(mut logs) = terminal_logs.lock() {
                            logs.push("[LAUNCH] Minecraft process finished.".to_string());
                        }
                    }
                    Err(e) => {
                        *status_text.lock().unwrap() = format!("Launch failed: {}", e);
                        write_crash_log(&format!("Launch failed: {}", e));
                        if let Ok(mut logs) = terminal_logs.lock() {
                            logs.push(format!("[ERROR] Launch failed: {}", e));
                        }
                    }
                }

                *is_launching.lock().unwrap() = false;
                ctx_refresh.request_repaint();
            });
        });
    }

    fn trigger_mod_search(&self, ctx: &egui::Context, target_db: &str) {
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            *self.status_text.lock().unwrap() = "Type a mod name before searching.".to_string();
            return;
        }

        *self.is_searching.lock().unwrap() = true;
        *self.status_text.lock().unwrap() = format!("Searching {} for {}...", target_db, query);

        let selected_version = self.selected_version.clone();
        let selected_loader = self.selected_loader.clone();
        let search_results = Arc::clone(&self.search_results);
        let is_searching = Arc::clone(&self.is_searching);
        let status_text = Arc::clone(&self.status_text);
        let terminal_logs = Arc::clone(&self.terminal_logs);
        let ctx_refresh = ctx.clone();
        let target_db = target_db.to_string();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let client = reqwest::Client::new();
                let mut facet_groups = vec![
                    r#"["project_type:mod"]"#.to_string(),
                    format!(r#"["versions:{}"]"#, selected_version),
                ];

                if let Some(loader_category) = match selected_loader.as_str() {
                    "Fabric" => Some("fabric"),
                    "Quilt" => Some("quilt"),
                    "Forge" => Some("forge"),
                    "NeoForge" => Some("neoforge"),
                    _ => None,
                } {
                    facet_groups.push(format!(r#"["categories:{}"]"#, loader_category));
                }

                let facets = format!("[{}]", facet_groups.join(","));
                let result = client
                    .get("https://api.modrinth.com/v2/search")
                    .header("User-Agent", "FusionLauncher/0.1.0")
                    .query(&[
                        ("query", query.as_str()),
                        ("limit", "12"),
                        ("facets", facets.as_str()),
                    ])
                    .send()
                    .await
                    .map_err(|e| format!("Search request failed: {}", e));

                match result {
                    Ok(response) => match response.error_for_status() {
                        Ok(response) => match response.json::<ModrinthSearchResponse>().await {
                            Ok(payload) => {
                                let hit_count = payload.hits.len();
                                *search_results.lock().unwrap() = payload.hits;
                                *status_text.lock().unwrap() =
                                    format!("Found {} results on {}.", hit_count, target_db);
                                if let Ok(mut logs) = terminal_logs.lock() {
                                    logs.push(format!(
                                        "[SEARCH] {} returned {} results for '{}' on {} {}.",
                                        target_db,
                                        hit_count,
                                        query,
                                        selected_loader,
                                        selected_version
                                    ));
                                }
                            }
                            Err(e) => {
                                *status_text.lock().unwrap() =
                                    format!("Search response could not be read: {}", e);
                                if let Ok(mut logs) = terminal_logs.lock() {
                                    logs.push(format!(
                                        "[ERROR] Search response parse failed: {}",
                                        e
                                    ));
                                }
                            }
                        },
                        Err(e) => {
                            *status_text.lock().unwrap() = format!("Search failed: {}", e);
                            if let Ok(mut logs) = terminal_logs.lock() {
                                logs.push(format!(
                                    "[ERROR] Search endpoint rejected request: {}",
                                    e
                                ));
                            }
                        }
                    },
                    Err(e) => {
                        *status_text.lock().unwrap() = e.clone();
                        if let Ok(mut logs) = terminal_logs.lock() {
                            logs.push(format!("[ERROR] {}", e));
                        }
                    }
                }

                *is_searching.lock().unwrap() = false;
                ctx_refresh.request_repaint();
            });
        });
    }

    fn trigger_shader_search(&self, ctx: &egui::Context) {
        let query = self.shader_search_query.trim().to_string();

        *self.is_shader_searching.lock().unwrap() = true;
        *self.status_text.lock().unwrap() = if query.is_empty() {
            "Searching Modrinth shaders...".to_string()
        } else {
            format!("Searching Modrinth shaders for {}...", query)
        };

        let selected_version = self.selected_version.clone();
        let shader_search_results = Arc::clone(&self.shader_search_results);
        let is_shader_searching = Arc::clone(&self.is_shader_searching);
        let status_text = Arc::clone(&self.status_text);
        let terminal_logs = Arc::clone(&self.terminal_logs);
        let ctx_refresh = ctx.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let client = reqwest::Client::new();
                let facets = format!(
                    r#"[["project_type:shader"],["versions:{}"],["categories:iris"]]"#,
                    selected_version
                );
                let result = client
                    .get("https://api.modrinth.com/v2/search")
                    .header("User-Agent", "FusionLauncher/0.1.0")
                    .query(&[
                        ("query", query.as_str()),
                        ("limit", "12"),
                        ("index", "downloads"),
                        ("facets", facets.as_str()),
                    ])
                    .send()
                    .await
                    .map_err(|e| format!("Shader search request failed: {}", e));

                match result {
                    Ok(response) => match response.error_for_status() {
                        Ok(response) => match response.json::<ModrinthSearchResponse>().await {
                            Ok(payload) => {
                                let hit_count = payload.hits.len();
                                *shader_search_results.lock().unwrap() = payload.hits;
                                *status_text.lock().unwrap() =
                                    format!("Found {} Iris shader results.", hit_count);
                                if let Ok(mut logs) = terminal_logs.lock() {
                                    logs.push(format!(
                                        "[SHADERS] Modrinth returned {} Iris shader results for '{}' on {}.",
                                        hit_count, query, selected_version
                                    ));
                                }
                            }
                            Err(e) => {
                                *status_text.lock().unwrap() =
                                    format!("Shader search response could not be read: {}", e);
                            }
                        },
                        Err(e) => {
                            *status_text.lock().unwrap() =
                                format!("Shader search failed: {}", e);
                        }
                    },
                    Err(e) => {
                        *status_text.lock().unwrap() = e.clone();
                        if let Ok(mut logs) = terminal_logs.lock() {
                            logs.push(format!("[ERROR] {}", e));
                        }
                    }
                }

                *is_shader_searching.lock().unwrap() = false;
                ctx_refresh.request_repaint();
            });
        });
    }

    fn trigger_mod_download(&mut self, ctx: &egui::Context, project_id: String, title: String) {
        let selected_version = self.selected_version.clone();
        let selected_loader = self.selected_loader.clone();
        let status_text = Arc::clone(&self.status_text);
        let terminal_logs = Arc::clone(&self.terminal_logs);
        let mods_dirty = Arc::clone(&self.mods_dirty);
        let ctx_refresh = ctx.clone();
        let game_dir = self.current_game_dir();
        let mods_dir = self.current_mods_dir();

        *status_text.lock().unwrap() = format!("Getting {}...", title);
        self.log_to_terminal(&format!(
            "Resolving install candidate for {} on {} {}...",
            title, selected_loader, selected_version
        ));

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let result = async {
                    prepare_isolated_instance(&game_dir)?;

                    let client = reqwest::Client::new();
                    install_modrinth_project_with_deps(
                        &client,
                        &mods_dir,
                        project_id,
                        &selected_version,
                        &selected_loader,
                        Arc::clone(&mods_dirty),
                    )
                    .await
                }
                .await;

                match result {
                    Ok(installed_files) => {
                        let install_count = installed_files.len();
                        *status_text.lock().unwrap() = format!(
                            "Installed {} mod{}.",
                            install_count,
                            if install_count == 1 { "" } else { "s" }
                        );
                        if let Ok(mut logs) = terminal_logs.lock() {
                            for filename in installed_files {
                                logs.push(format!("[MODS] Installed {} from Modrinth.", filename));
                            }
                        }
                    }
                    Err(e) => {
                        *status_text.lock().unwrap() = e.clone();
                        if let Ok(mut logs) = terminal_logs.lock() {
                            logs.push(format!("[ERROR] {}", e));
                        }
                    }
                }

                *mods_dirty.lock().unwrap() = true;
                ctx_refresh.request_repaint();
            });
        });
    }

    fn trigger_iris_download(&mut self, ctx: &egui::Context) {
        if !self.iris_supported_for_selected_loader() {
            *self.status_text.lock().unwrap() =
                "Iris is available here for Fabric, Quilt, and NeoForge profiles.".to_string();
            return;
        }

        self.trigger_mod_download(ctx, "iris".to_string(), "Iris Shaders".to_string());
    }

    fn trigger_shader_download(&mut self, ctx: &egui::Context, project_id: String, title: String) {
        if !self.iris_supported_for_selected_loader() {
            *self.status_text.lock().unwrap() =
                "Switch to Fabric, Quilt, or NeoForge and install Iris before using shaders."
                    .to_string();
            return;
        }

        let selected_version = self.selected_version.clone();
        let status_text = Arc::clone(&self.status_text);
        let terminal_logs = Arc::clone(&self.terminal_logs);
        let ctx_refresh = ctx.clone();
        let game_dir = self.current_game_dir();
        let shaderpacks_dir = self.current_shaderpacks_dir();

        *status_text.lock().unwrap() = format!("Getting shader {}...", title);
        self.log_to_terminal(&format!(
            "Resolving Iris shaderpack candidate for {} on {}...",
            title, selected_version
        ));

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let result = async {
                    prepare_isolated_instance(&game_dir)?;

                    let client = reqwest::Client::new();
                    install_modrinth_shader_project(
                        &client,
                        &shaderpacks_dir,
                        project_id,
                        &selected_version,
                    )
                    .await
                }
                .await;

                match result {
                    Ok(filename) => {
                        *status_text.lock().unwrap() =
                            format!("Installed shaderpack {}.", filename);
                        if let Ok(mut logs) = terminal_logs.lock() {
                            logs.push(format!("[SHADERS] Installed {} from Modrinth.", filename));
                        }
                    }
                    Err(e) => {
                        *status_text.lock().unwrap() = e.clone();
                        if let Ok(mut logs) = terminal_logs.lock() {
                            logs.push(format!("[ERROR] {}", e));
                        }
                    }
                }

                ctx_refresh.request_repaint();
            });
        });
    }

    fn trigger_microsoft_auth(&self, ctx: &egui::Context) {
        *self.is_authenticating.lock().unwrap() = true;
        *self.auth_cancel_token.lock().unwrap() = false;
        *self.auth_refresh_trigger.lock().unwrap() = false;
        *self.status_text.lock().unwrap() = "Connecting to auth servers...".to_string();

        let status_clone = Arc::clone(&self.status_text);
        let auth_lock = Arc::clone(&self.is_authenticating);
        let profile_clone = Arc::clone(&self.ms_profile);
        let logs_clone = Arc::clone(&self.terminal_logs);
        let cancel_clone = Arc::clone(&self.auth_cancel_token);
        let refresh_clone = Arc::clone(&self.auth_refresh_trigger);
        let ctx_refresh = ctx.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {

                let auth_manager = CustomMicrosoftAuth::new("25d081b2-e919-4a82-9ec7-5c753d654fb5");

                match auth_manager.authenticate_device(status_clone.clone(), logs_clone.clone(), ctx_refresh.clone(), cancel_clone.clone(), refresh_clone.clone()).await {
                    Ok(profile) => {
                        if let Ok(mut logs) = logs_clone.lock() {
                            logs.push(format!("[AUTH] Official session validated for user profile: {}", profile.username));
                            match save_cached_microsoft_profile(&profile) {
                                Ok(()) => logs.push("[AUTH] Microsoft session cached for future launches.".to_string()),
                                Err(e) => logs.push(format!("[ERROR] {}", e)),
                            }
                        }
                        *status_clone.lock().unwrap() = format!("Welcome back, {}!", profile.username);
                        *profile_clone.lock().unwrap() = Some(profile);
                    }
                    Err(e) => {
                        if *refresh_clone.lock().unwrap() {
                            if let Ok(mut logs) = logs_clone.lock() {
                                logs.push("[AUTH] Actively recycling OAuth endpoint device code strings...".to_string());
                            }
                            *refresh_clone.lock().unwrap() = false;
                            *auth_lock.lock().unwrap() = false;

                            ctx_refresh.request_repaint();
                            return;
                        }

                        if let Ok(mut logs) = logs_clone.lock() {
                            logs.push(format!("[ERROR] Microsoft OAuth rejected: {}", e));
                        }
                        if *cancel_clone.lock().unwrap() {
                            *status_clone.lock().unwrap() = "Authentication canceled by user.".to_string();
                        } else {
                            *status_clone.lock().unwrap() = format!("Microsoft Login Failed: {}", e);
                        }
                    }
                }
                *auth_lock.lock().unwrap() = false;
                ctx_refresh.request_repaint();
            });
        });
    }

    fn refresh_mods_list(&mut self) {
        let mods_dir = self.current_mods_dir();
        if !mods_dir.exists() {
            let _ = fs::create_dir_all(&mods_dir);
        }

        let mut current_scanned_mods = Vec::new();

        if let Ok(entries) = fs::read_dir(&mods_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        let filename = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string();

                        if ext == "jar" {
                            if self.selected_loader != "Vanilla" {
                                current_scanned_mods.push((filename, true));
                            }
                        } else if ext == "bak" {
                            if filename.ends_with(".jar.bak") {
                                let clean_name = filename
                                    .strip_suffix(".bak")
                                    .unwrap_or(&filename)
                                    .to_string();
                                if self.selected_loader != "Vanilla" {
                                    current_scanned_mods.push((clean_name, false));
                                }
                            }
                        }
                    }
                }
            }
        }

        if self.found_mods != current_scanned_mods {
            self.found_mods = current_scanned_mods;
        }
        self.last_folder_scan = Instant::now();
    }

    fn run_mod_compatibility_check(&mut self) {
        self.refresh_mods_list();
        self.compatibility_results.clear();

        let enabled_mods = self
            .found_mods
            .iter()
            .filter(|(_, enabled)| *enabled)
            .map(|(name, _)| (name.clone(), normalized_mod_name(name)))
            .collect::<Vec<_>>();

        if enabled_mods.is_empty() {
            self.compatibility_results.push(CompatibilityResult {
                kind: CompatibilityResultKind::Info,
                message: "No enabled mods to check for this profile.".to_string(),
            });
            self.log_to_terminal("Compatibility check found no enabled mods.");
            *self.status_text.lock().unwrap() = "No enabled mods to check.".to_string();
            return;
        }

        let find_mod = |aliases: &[&str]| {
            enabled_mods
                .iter()
                .find(|(_, normalized)| aliases.iter().any(|alias| normalized.contains(alias)))
        };

        const CONFLICT_RULES: &[(&[&str], &[&str], &str)] = &[
            (
                &["betterend", "betterx"],
                &["sodium"],
                "BetterEnd/BetterX and Sodium are a known bad rendering/worldgen combo. Disable one before playing.",
            ),
            (
                &["optifine"],
                &["sodium"],
                "OptiFine and Sodium both replace major rendering systems. Use one rendering stack.",
            ),
            (
                &["optifine"],
                &["iris"],
                "OptiFine and Iris both handle shader/render integration. Use Iris or OptiFine, not both.",
            ),
            (
                &["sodium"],
                &["rubidium", "embeddium"],
                "Sodium, Rubidium, and Embeddium are alternate renderer mods. Keep only the one for your loader.",
            ),
            (
                &["rubidium"],
                &["embeddium"],
                "Rubidium and Embeddium replace the same renderer path. Keep only one.",
            ),
            (
                &["iris"],
                &["oculus"],
                "Iris and Oculus are shader mods for different loaders. Keep only the one for your loader.",
            ),
            (
                &["phosphor"],
                &["starlight"],
                "Phosphor and Starlight both replace the lighting engine. Keep only one lighting mod.",
            ),
        ];

        for (left_aliases, right_aliases, message) in CONFLICT_RULES {
            if let (Some((left_name, _)), Some((right_name, _))) =
                (find_mod(left_aliases), find_mod(right_aliases))
            {
                self.compatibility_results.push(CompatibilityResult {
                    kind: CompatibilityResultKind::Error,
                    message: format!(
                        "{} + {}: {}",
                        mod_display_name(left_name),
                        mod_display_name(right_name),
                        message
                    ),
                });
            }
        }

        const REQUIRED_RULES: &[(&[&str], &[&str], &str)] = &[
            (
                &["indium"],
                &["sodium"],
                "Indium is meant to run with Sodium. Install/enable Sodium or disable Indium.",
            ),
            (
                &["oculus"],
                &["rubidium", "embeddium"],
                "Oculus normally needs Rubidium or Embeddium on Forge/NeoForge.",
            ),
        ];

        for (mod_aliases, dependency_aliases, message) in REQUIRED_RULES {
            if let Some((mod_name, _)) = find_mod(mod_aliases) {
                if find_mod(dependency_aliases).is_none() {
                    self.compatibility_results.push(CompatibilityResult {
                        kind: CompatibilityResultKind::Warning,
                        message: format!("{}: {}", mod_display_name(mod_name), message),
                    });
                }
            }
        }

        if self.compatibility_results.is_empty() {
            self.compatibility_results.push(CompatibilityResult {
                kind: CompatibilityResultKind::Success,
                message: "No known compatibility issues found for the enabled mods.".to_string(),
            });
        }

        let has_errors = self.compatibility_results.iter().any(|result| {
            matches!(
                result.kind,
                CompatibilityResultKind::Error | CompatibilityResultKind::Warning
            )
        });
        self.log_to_terminal(if has_errors {
            "Compatibility check found mod conflicts."
        } else {
            "Compatibility check completed without known conflicts."
        });
        *self.status_text.lock().unwrap() = if has_errors {
            "Compatibility issues found.".to_string()
        } else {
            "Compatibility check complete.".to_string()
        };
    }

    fn toggle_mod(&mut self, idx: usize) {
        let mods_dir = self.current_mods_dir();
        if idx < self.found_mods.len() {
            let (mod_name, currently_enabled) = self.found_mods[idx].clone();
            let src_path: PathBuf;
            let dest_path: PathBuf;

            if currently_enabled {
                src_path = mods_dir.join(&mod_name);
                dest_path = mods_dir.join(format!("{}.bak", mod_name));
                self.log_to_terminal(&format!("User manually toggled OFF mod: {}", mod_name));
            } else {
                src_path = mods_dir.join(format!("{}.bak", mod_name));
                dest_path = mods_dir.join(&mod_name);
                self.log_to_terminal(&format!("User manually toggled ON mod: {}", mod_name));
            }
            if fs::rename(src_path, dest_path).is_ok() {
                if let Some((_, enabled)) = self.found_mods.get_mut(idx) {
                    *enabled = !currently_enabled;
                }
                self.last_folder_scan = Instant::now();
            }
        }
    }

    fn is_loader_supported_for_version(&self, loader: &str, version: &str) -> bool {
        if let Some(availability) = self.loader_availability.lock().unwrap().get(version) {
            return availability.get(loader).copied().unwrap_or(false);
        }

        fallback_loader_supported_for_version(loader, version)
    }

    fn is_loader_supported(&self, loader: &str) -> bool {
        self.is_loader_supported_for_version(loader, &self.selected_version)
    }

    fn ensure_loader_availability_check(&mut self, ctx: &egui::Context) {
        let version = self.selected_version.clone();
        if self
            .loader_availability
            .lock()
            .unwrap()
            .contains_key(&version)
        {
            return;
        }
        if self.last_loader_availability_request == version
            || *self.is_checking_loader_availability.lock().unwrap()
        {
            return;
        }

        self.last_loader_availability_request = version.clone();
        *self.is_checking_loader_availability.lock().unwrap() = true;

        let loader_availability = Arc::clone(&self.loader_availability);
        let is_checking = Arc::clone(&self.is_checking_loader_availability);
        let ctx_refresh = ctx.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let mut availability = HashMap::new();
                availability.insert("Vanilla".to_string(), true);

                for loader in ["Fabric", "Quilt", "Forge", "NeoForge"] {
                    let supported = resolve_loader_version(loader, &version).await.is_ok();
                    availability.insert(loader.to_string(), supported);
                }

                loader_availability
                    .lock()
                    .unwrap()
                    .insert(version, availability);
                *is_checking.lock().unwrap() = false;
                ctx_refresh.request_repaint();
            });
        });
    }

    fn show_mod_profile_selector(&mut self, ui: &mut egui::Ui, id_source: &str) {
        let old_profile = self.selected_mod_profile;
        egui::ComboBox::from_id_source(id_source)
            .selected_text(mod_profile_name(self.selected_mod_profile))
            .width(130.0)
            .show_ui(ui, |ui| {
                for profile in 1..=MAX_MOD_PROFILES {
                    ui.selectable_value(
                        &mut self.selected_mod_profile,
                        profile,
                        mod_profile_name(profile),
                    );
                }
            });

        self.selected_mod_profile = clamp_mod_profile(self.selected_mod_profile);
        if old_profile != self.selected_mod_profile {
            self.compatibility_results.clear();
            let _ = self.migrate_legacy_mods_for_current_environment();
            self.refresh_mods_list();
            self.log_to_terminal(&format!(
                "Switched to {}.",
                mod_profile_name(self.selected_mod_profile)
            ));
        }
    }
}

impl eframe::App for FusionLauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.taskbar_icon_applied {
            ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(load_app_icon()))));
            self.taskbar_icon_applied = true;
        }

        if !*self.is_launching.lock().unwrap() {
            self.launch_started_at = None;
        }

        let microsoft_username = self
            .ms_profile
            .lock()
            .unwrap()
            .as_ref()
            .map(|profile| profile.username.clone());
        if self.auth_mode == AuthMode::Microsoft {
            if let Some(username) = &microsoft_username {
                if self.username != *username {
                    self.username = username.clone();
                }
            }
        }

        self.ensure_loader_availability_check(ctx);
        if !self.is_loader_supported(&self.selected_loader) {
            let previous_loader = self.selected_loader.clone();
            self.selected_loader = "Vanilla".to_string();
            self.log_to_terminal(&format!(
                "{} is not available for {}. Switched to Vanilla.",
                previous_loader, self.selected_version
            ));
            let _ = self.migrate_legacy_mods_for_current_environment();
            self.refresh_mods_list();
        }

        let should_refresh_mods = {
            let mut mods_dirty = self.mods_dirty.lock().unwrap();
            if *mods_dirty {
                *mods_dirty = false;
                true
            } else {
                false
            }
        };
        if should_refresh_mods
            || (self.current_tab == ActiveTab::Mods
                && self.last_folder_scan.elapsed() > Duration::from_secs(2))
        {
            self.refresh_mods_list();
        }

        self.save_config_if_changed();

        // Handle clean polling sync transitions for code refresh commands
        if *self.is_authenticating.lock().unwrap() == false
            && *self.auth_refresh_trigger.lock().unwrap() == true
        {
            self.trigger_microsoft_auth(ctx);
        }

        // Direct evaluation via self inside the main sync update loop
        if self.auth_mode == AuthMode::Microsoft && self.ms_profile.lock().unwrap().is_none() {
            if !*self.is_authenticating.lock().unwrap() {
                // Auto-trigger auth loop if context defaults to Microsoft mode without active profile
                self.trigger_microsoft_auth(ctx);
            }
        }

        ctx.request_repaint_after(Duration::from_millis(200));

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("FUSION LAUNCHER")
                        .strong()
                        .size(20.0)
                        .color(egui::Color32::from_rgb(187, 154, 247)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let authenticating = *self.is_authenticating.lock().unwrap();

                    if authenticating {
                        ui.add(egui::Spinner::new().size(16.0));

                        let refresh_btn = ui.add(
                            egui::Button::new(egui::RichText::new("🔄 Refresh Code").small().strong())
                                .fill(egui::Color32::from_rgb(74, 85, 104))
                        );
                        if refresh_btn.clicked() {
                            *self.auth_refresh_trigger.lock().unwrap() = true;
                        }

                        if ui.small_button("Cancel").clicked() {
                            self.auth_mode = AuthMode::Offline;
                            *self.auth_cancel_token.lock().unwrap() = true;
                            *self.is_authenticating.lock().unwrap() = false;
                            *self.status_text.lock().unwrap() =
                                "Authentication canceled by user.".to_string();
                            self.log_to_terminal("Microsoft authentication canceled. Reverted to offline mode.");
                        }
                    } else {
                        match self.auth_mode {
                            AuthMode::Offline => {
                                let ms_btn = ui.add(
                                    egui::Button::new(egui::RichText::new("🌐 Microsoft Login").small().strong())
                                        .fill(egui::Color32::from_rgb(0, 164, 239))
                                );
                                if ms_btn.clicked() {
                                    self.auth_mode = AuthMode::Microsoft;
                                    self.trigger_microsoft_auth(ctx);
                                }
                            }
                            AuthMode::Microsoft => {
                                let active_name = if let Some(p) = &*self.ms_profile.lock().unwrap() {
                                    p.username.clone()
                                } else {
                                    "Official Account".to_string()
                                };

                                ui.label(egui::RichText::new(format!("🟢 {}", active_name)).strong().color(egui::Color32::from_rgb(162, 210, 166)));
                                if ui.small_button("Log Out").clicked() {
                                    self.logout_microsoft();
                                }
                            }
                        }
                    }
                });
            });
            ui.add_space(8.0);

            // TAB NAVIGATION BAR
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, ActiveTab::Play, "🎮 Play");
                ui.selectable_value(&mut self.current_tab, ActiveTab::Mods, "📦 Mods");
                ui.selectable_value(&mut self.current_tab, ActiveTab::Skins, "👕 Skins");
                ui.selectable_value(&mut self.current_tab, ActiveTab::Shaders, "Shaders");
                ui.selectable_value(&mut self.current_tab, ActiveTab::Settings, "⚙ Settings");
            });
            ui.separator();
            ui.add_space(5.0);

            match self.current_tab {
                ActiveTab::Play => {
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());

                        // 🌐 Explicit Online/Offline Account Switch Mode
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Account Mode:").strong());
                            ui.radio_value(&mut self.auth_mode, AuthMode::Offline, "Offline");
                            ui.radio_value(
                                &mut self.auth_mode,
                                AuthMode::Microsoft,
                                "Microsoft (Online)",
                            );
                        });
                        ui.add_space(6.0);

                        ui.horizontal(|ui| {
                            ui.label("Username:");
                            if self.auth_mode == AuthMode::Microsoft {
                                let active_name = microsoft_username
                                    .clone()
                                    .unwrap_or_else(|| "Authentication Pending...".to_string());
                                ui.label(egui::RichText::new(active_name).color(egui::Color32::LIGHT_GRAY).italics());
                            } else {
                                ui.text_edit_singleline(&mut self.username);
                            }
                        });
                        ui.add_space(4.0);

                        // 🎮 Game Profile Versions
                        ui.horizontal(|ui| {
                            ui.label("Minecraft:");
                            let old_version = self.selected_version.clone();
                            egui::ComboBox::from_id_source("mc_version")
                                .selected_text(&self.selected_version)
                                .width(130.0)
                                .show_ui(ui, |ui| {
                                    for v in MINECRAFT_VERSIONS {
                                        ui.selectable_value(&mut self.selected_version, v.to_string(), *v);
                                    }
                                });
                            if old_version != self.selected_version {
                                self.ensure_loader_availability_check(ctx);
                                if !self.is_loader_supported(&self.selected_loader) {
                                    self.selected_loader = "Vanilla".to_string();
                                }
                                let _ = self.migrate_legacy_mods_for_current_environment();
                                self.refresh_mods_list();
                            }
                        });
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Mod Engine:");
                            let old_loader = self.selected_loader.clone();
                            egui::ComboBox::from_id_source("loader_type")
                                .selected_text(&self.selected_loader)
                                .width(130.0)
                                .show_ui(ui, |ui| {
                                    if self.is_loader_supported("Fabric") { ui.selectable_value(&mut self.selected_loader, "Fabric".to_string(), "Fabric 🟢"); }
                                    if self.is_loader_supported("Quilt") { ui.selectable_value(&mut self.selected_loader, "Quilt".to_string(), "Quilt 🟣"); }
                                    if self.is_loader_supported("Forge") { ui.selectable_value(&mut self.selected_loader, "Forge".to_string(), "Forge 🧡"); }
                                    if self.is_loader_supported("NeoForge") { ui.selectable_value(&mut self.selected_loader, "NeoForge".to_string(), "NeoForge 🔥"); }
                                    if self.is_loader_supported("Vanilla") { ui.selectable_value(&mut self.selected_loader, "Vanilla".to_string(), "Vanilla 🟥"); }
                                });
                            if *self.is_checking_loader_availability.lock().unwrap() {
                                ui.add(egui::Spinner::new().size(12.0));
                            }
                            if old_loader != self.selected_loader {
                                self.log_to_terminal(&format!("Mod loader target environment changed to: {}", self.selected_loader));
                                let _ = self.migrate_legacy_mods_for_current_environment();
                                self.refresh_mods_list();
                            }
                        });
                    });
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| { // Fixed typo method layout name hook
                        let launch_btn_text = if *self.is_launching.lock().unwrap() { "Running Client..." } else { "Play" };
                        let launch_btn = ui.add_enabled(
                            !*self.is_launching.lock().unwrap(),
                            egui::Button::new(egui::RichText::new(launch_btn_text).strong().size(16.0))
                                .min_size(egui::vec2(240.0, 44.0))
                                .fill(egui::Color32::from_rgb(187, 154, 247))
                        );
                        if launch_btn.clicked() {
                            self.trigger_game_launch(ctx);
                        }

                        if *self.is_launching.lock().unwrap() {
                            ui.add_space(10.0);
                            let elapsed = self
                                .launch_started_at
                                .map(|started| started.elapsed().as_secs())
                                .unwrap_or(0);
                            let progress =
                                (elapsed as f32 / EXPECTED_LAUNCH_SECONDS as f32).clamp(0.03, 1.0);
                            let percent = (progress * 100.0).round() as u32;
                            let progress_text = if percent >= 100 {
                                "Done.".to_string()
                            } else {
                                format!("Preparing game... {}%", percent)
                            };

                            ui.add(
                                egui::ProgressBar::new(progress)
                                    .animate(true)
                                    .desired_width(260.0)
                                    .text(progress_text),
                            );
                        }
                    });
                }

                ActiveTab::Mods => {
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Local Instance Mods").strong());
                            self.show_mod_profile_selector(ui, "mods_tab_profile");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("📂 Open Folder").clicked() {
                                    let mods_dir = self.current_mods_dir();
                                    let _ = std::process::Command::new("explorer").arg(mods_dir).spawn();
                                }

                                if ui.button("Check Compatibility").clicked() {
                                    self.run_mod_compatibility_check();
                                }
                            });
                        });
                        ui.add_space(4.0);

                        for result in &self.compatibility_results {
                            let color = match result.kind {
                                CompatibilityResultKind::Error => {
                                    egui::Color32::from_rgb(239, 83, 80)
                                }
                                CompatibilityResultKind::Warning => {
                                    egui::Color32::from_rgb(255, 202, 40)
                                }
                                CompatibilityResultKind::Success => {
                                    egui::Color32::from_rgb(102, 187, 106)
                                }
                                CompatibilityResultKind::Info => {
                                    egui::Color32::from_rgb(189, 189, 189)
                                }
                            };
                            ui.label(egui::RichText::new(&result.message).color(color).strong());
                        }

                        if !self.compatibility_results.is_empty() {
                            ui.add_space(4.0);
                        }

                        egui::ScrollArea::vertical().id_source("local_mods_scroll").max_height(140.0).show(ui, |ui| {
                            if self.found_mods.is_empty() {
                                ui.label(egui::RichText::new("No compatible mods loaded for this profile.").weak().italics());
                            } else {
                                for i in 0..self.found_mods.len() {
                                    let (mod_name, enabled) = self.found_mods[i].clone();
                                    ui.horizontal(|ui| {
                                        let mut is_enabled = enabled;
                                        if ui.checkbox(&mut is_enabled, "").changed() {
                                            self.toggle_mod(i);
                                        }
                                        let display_name = mod_display_name(&mod_name);
                                        let label = if enabled {
                                            egui::RichText::new(display_name)
                                        } else {
                                            egui::RichText::new(display_name)
                                                .color(egui::Color32::from_rgb(239, 83, 80))
                                                .strikethrough()
                                                .weak()
                                        };
                                        ui.label(label).on_hover_text(if enabled {
                                            format!("Enabled for this profile.\n{}", mod_name)
                                        } else {
                                            format!("Disabled. Check it to enable this mod for the selected profile.\n{}", mod_name)
                                        });
                                    });
                                }
                            }
                        });
                    });

                    ui.add_space(10.0);

                    // 📦 Dynamic Database Architecture Mapping
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());

                        let target_db = "Modrinth";

                        ui.label(egui::RichText::new(format!("Discover Content on {} ({})", target_db, self.selected_loader)).strong());
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut self.search_query);
                            let search_btn_text = if *self.is_searching.lock().unwrap() { "Searching...".to_string() } else { format!("Search {}", target_db) };

                            if ui.button(search_btn_text).clicked() && !*self.is_searching.lock().unwrap() {
                                self.log_to_terminal(&format!("Dispatching payload querying task directly to {} endpoints for context: {}", target_db, self.search_query));
                                self.trigger_mod_search(ctx, target_db);
                            }
                        });

                        ui.add_space(8.0);
                        egui::ScrollArea::vertical()
                            .id_source("search_results_scroll")
                            .max_height(170.0)
                            .show(ui, |ui| {
                                let results = self.search_results.lock().unwrap().clone();
                                if results.is_empty() {
                                    ui.label(
                                        egui::RichText::new("Search results will appear here.")
                                            .weak()
                                            .italics(),
                                    );
                                } else {
                                    for result in results {
                                        ui.group(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    egui::RichText::new(&result.title).strong(),
                                                );
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        let is_installed = is_project_installed_for_profile(
                                                            &result.project_id,
                                                            &result.slug,
                                                            &result.title,
                                                            &self.selected_version,
                                                            &self.selected_loader,
                                                            self.selected_mod_profile,
                                                        );

                                                        if is_installed {
                                                            ui.add_enabled(
                                                                false,
                                                                egui::Button::new("Already Installed"),
                                                            );
                                                        } else if ui.small_button("Get").clicked() {
                                                            self.trigger_mod_download(
                                                                ctx,
                                                                result.project_id.clone(),
                                                                result.title.clone(),
                                                            );
                                                        }
                                                    },
                                                );
                                            });
                                            ui.label(
                                                egui::RichText::new(&result.description)
                                                    .weak()
                                                    .small(),
                                            );
                                            ui.label(
                                                egui::RichText::new(format!("Project ID: {}", result.project_id))
                                                    .monospace()
                                                    .small(),
                                            );
                                        });
                                        ui.add_space(4.0);
                                    }
                                }
                            });
                    });
                }

                ActiveTab::Skins => {
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("Cosmetic Visual Configurations").strong());
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Local Path:");
                            ui.text_edit_singleline(&mut self.skin_path_input);
                        });
                        ui.add_space(6.0);
                        if ui.button("Apply Skin Context").clicked() {
                            self.log_to_terminal("Re-mapping offline asset path structures for customized skin resolution...");
                        }
                    });
                }

                ActiveTab::Shaders => {
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Iris Shader Packs").strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Open Shaderpacks Folder").clicked() {
                                    let shaderpacks_dir = self.current_shaderpacks_dir();
                                    let _ = std::process::Command::new("explorer")
                                        .arg(shaderpacks_dir)
                                        .spawn();
                                }
                            });
                        });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let iris_installed = is_manifest_project_installed_for_profile(
                                "iris",
                                "iris",
                                "Iris Shaders",
                                &self.selected_version,
                                &self.selected_loader,
                                self.selected_mod_profile,
                            );
                            let iris_text = if iris_installed {
                                "Iris Installed"
                            } else {
                                "Download Iris"
                            };

                            if ui
                                .add_enabled(
                                    self.iris_supported_for_selected_loader() && !iris_installed,
                                    egui::Button::new(iris_text),
                                )
                                .clicked()
                            {
                                self.trigger_iris_download(ctx);
                            }

                            if !self.iris_supported_for_selected_loader() {
                                ui.label(
                                    egui::RichText::new(
                                        "Use Fabric, Quilt, or NeoForge for Iris shaders.",
                                    )
                                    .color(egui::Color32::from_rgb(255, 202, 40)),
                                );
                            } else if !iris_installed {
                                ui.label(
                                    egui::RichText::new(
                                        "Iris is required before shader packs will work.",
                                    )
                                    .color(egui::Color32::from_rgb(255, 202, 40)),
                                );
                            }
                        });
                    });

                    ui.add_space(10.0);

                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            egui::RichText::new(format!(
                                "Search Modrinth Shaders ({})",
                                self.selected_version
                            ))
                            .strong(),
                        );
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut self.shader_search_query);
                            let search_btn_text = if *self.is_shader_searching.lock().unwrap() {
                                "Searching...".to_string()
                            } else {
                                "Search Shaders".to_string()
                            };

                            if ui.button(search_btn_text).clicked()
                                && !*self.is_shader_searching.lock().unwrap()
                            {
                                self.trigger_shader_search(ctx);
                            }
                        });

                        ui.hyperlink_to(
                            "Open Modrinth Shaders",
                            "https://modrinth.com/discover/shaders",
                        );

                        ui.add_space(8.0);
                        egui::ScrollArea::vertical()
                            .id_source("shader_results_scroll")
                            .max_height(250.0)
                            .show(ui, |ui| {
                                let results = self.shader_search_results.lock().unwrap().clone();
                                if results.is_empty() {
                                    ui.label(
                                        egui::RichText::new("Shader results will appear here.")
                                            .weak()
                                            .italics(),
                                    );
                                } else {
                                    for result in results {
                                        ui.group(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    egui::RichText::new(&result.title).strong(),
                                                );
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        let is_installed =
                                                            is_shader_installed_for_profile(
                                                                &result.project_id,
                                                                &result.slug,
                                                                &result.title,
                                                                &self.selected_version,
                                                                &self.selected_loader,
                                                                self.selected_mod_profile,
                                                            );

                                                        if is_installed {
                                                            ui.add_enabled(
                                                                false,
                                                                egui::Button::new(
                                                                    "Already Installed",
                                                                ),
                                                            );
                                                        } else if ui.small_button("Get").clicked() {
                                                            self.trigger_shader_download(
                                                                ctx,
                                                                result.project_id.clone(),
                                                                result.title.clone(),
                                                            );
                                                        }
                                                    },
                                                );
                                            });
                                            ui.label(
                                                egui::RichText::new(&result.description)
                                                    .weak()
                                                    .small(),
                                            );
                                        });
                                        ui.add_space(4.0);
                                    }
                                }
                            });
                    });
                }

                ActiveTab::Settings => {
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("Account").strong());
                        ui.add_space(4.0);

                        let microsoft_name = self
                            .ms_profile
                            .lock()
                            .unwrap()
                            .as_ref()
                            .map(|profile| profile.username.clone());

                        ui.horizontal(|ui| {
                            ui.label("Microsoft:");
                            match microsoft_name {
                                Some(name) => {
                                    ui.label(
                                        egui::RichText::new(name)
                                            .color(egui::Color32::from_rgb(162, 210, 166))
                                            .strong(),
                                    );
                                    if ui.button("Log Out").clicked() {
                                        self.logout_microsoft();
                                    }
                                }
                                None => {
                                    ui.label(egui::RichText::new("Not signed in").weak());
                                    if ui.button("Sign In").clicked() {
                                        self.auth_mode = AuthMode::Microsoft;
                                        self.trigger_microsoft_auth(ctx);
                                    }
                                }
                            }
                        });
                    });

                    ui.add_space(10.0);

                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("Java Runtime").strong());
                        ui.add_space(6.0);

                        let java_status = self.java_status.lock().unwrap().clone();
                        let is_installing_java = *self.is_installing_java.lock().unwrap();

                        ui.horizontal(|ui| {
                            ui.label("Status:");
                            ui.label(egui::RichText::new(java_status).weak());
                        });

                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(!is_installing_java, egui::Button::new("Check Java"))
                                .clicked()
                            {
                                *self.java_status.lock().unwrap() =
                                    "Checking Java runtime...".to_string();
                                self.check_java_runtime(ctx);
                            }

                            let install_text = if is_installing_java {
                                "Installing..."
                            } else {
                                "Install Java"
                            };
                            if ui
                                .add_enabled(!is_installing_java, egui::Button::new(install_text))
                                .clicked()
                            {
                                self.install_java_runtime(ctx);
                            }
                        });
                    });

                    ui.add_space(10.0);

                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("Performance").strong());
                        ui.add_space(6.0);

                        let max_ram_gb = system_ram_limit_gb();
                        let max_cores = system_cpu_limit();
                        self.allocated_ram_gb = self.allocated_ram_gb.clamp(1, max_ram_gb);
                        self.cpu_cores = self.cpu_cores.clamp(1, max_cores);

                        ui.horizontal(|ui| {
                            ui.label("Allocated RAM:");
                            ui.add(
                                egui::Slider::new(&mut self.allocated_ram_gb, 1..=max_ram_gb)
                                    .suffix(" GB")
                                    .clamp_to_range(true),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label("CPU cores:");
                            ui.add(
                                egui::Slider::new(&mut self.cpu_cores, 1..=max_cores)
                                    .suffix(" cores")
                                    .clamp_to_range(true),
                            );
                        });

                        ui.label(
                            egui::RichText::new(format!(
                                "Detected system limits: {} GB RAM, {} CPU cores.",
                                max_ram_gb, max_cores
                            ))
                            .small()
                            .weak(),
                        );
                    });

                    ui.add_space(10.0);

                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("Graphics").strong());
                        ui.add_space(6.0);

                        ui.checkbox(&mut self.use_dedicated_gpu, "Prefer dedicated GPU");
                        ui.checkbox(
                            &mut self.enable_gpu_optimizations,
                            "Enable GPU optimization JVM flags",
                        );
                    });

                    ui.add_space(10.0);

                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("Advanced JVM Args").strong());
                        ui.add_space(6.0);
                        ui.text_edit_multiline(&mut self.custom_jvm_args);
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(self.launch_settings_summary())
                                .small()
                                .weak(),
                        );
                    });
                }
            }

            // Bottom Core Status Messages Layout Block
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.add_space(4.0);
                let current_status = self.status_text.lock().unwrap().clone();
                ui.label(egui::RichText::new(format!("Status: {}", current_status)).small().color(egui::Color32::LIGHT_GRAY));
            });
        });
    }
}
