$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")
$sourcePath = Join-Path $repoRoot "src\main.rs"
$targetPath = Join-Path $repoRoot "LINUX\src\main.rs"

if (-not (Test-Path -LiteralPath $sourcePath)) {
    throw "Source file not found: $sourcePath"
}
if (-not (Test-Path -LiteralPath (Split-Path -Parent $targetPath))) {
    throw "Linux source folder not found. Create the LINUX port first."
}

$content = Get-Content -LiteralPath $sourcePath -Raw

function Find-MatchingBrace {
    param(
        [string]$Text,
        [int]$OpenBraceIndex
    )

    $depth = 0
    for ($i = $OpenBraceIndex; $i -lt $Text.Length; $i++) {
        $ch = $Text[$i]
        if ($ch -eq '{') {
            $depth++
        } elseif ($ch -eq '}') {
            $depth--
            if ($depth -eq 0) {
                return $i
            }
        }
    }

    throw "Could not find matching brace."
}

function Replace-Function {
    param(
        [string]$Text,
        [string]$FunctionName,
        [string]$Replacement
    )

    $match = [regex]::Match($Text, "(?m)^\s*fn\s+$([regex]::Escape($FunctionName))\s*\(")
    if (-not $match.Success) {
        throw "Function not found: $FunctionName"
    }

    $openBrace = $Text.IndexOf("{", $match.Index)
    if ($openBrace -lt 0) {
        throw "Opening brace not found for function: $FunctionName"
    }

    $closeBrace = Find-MatchingBrace -Text $Text -OpenBraceIndex $openBrace
    return $Text.Remove($match.Index, $closeBrace - $match.Index + 1).Insert($match.Index, $Replacement.TrimEnd())
}

function Replace-Ram-Function-Block {
    param(
        [string]$Text,
        [string]$Replacement
    )

    $windowsCfg = [regex]::Match($Text, "(?m)^#\[cfg\(windows\)\]\s*\r?\n\s*fn\s+system_ram_limit_gb\s*\(")
    if (-not $windowsCfg.Success) {
        return Replace-Function -Text $Text -FunctionName "system_ram_limit_gb" -Replacement $Replacement
    }

    $firstOpenBrace = $Text.IndexOf("{", $windowsCfg.Index)
    if ($firstOpenBrace -lt 0) {
        throw "Opening brace not found for Windows system_ram_limit_gb."
    }

    $firstCloseBrace = Find-MatchingBrace -Text $Text -OpenBraceIndex $firstOpenBrace
    $notWindowsCfg = [regex]::Match($Text.Substring($firstCloseBrace + 1), "(?m)^\s*#\[cfg\(not\(windows\)\)\]\s*\r?\n\s*fn\s+system_ram_limit_gb\s*\(")
    if (-not $notWindowsCfg.Success) {
        throw "Non-Windows system_ram_limit_gb block not found."
    }

    $secondStart = $firstCloseBrace + 1 + $notWindowsCfg.Index
    $secondOpenBrace = $Text.IndexOf("{", $secondStart)
    if ($secondOpenBrace -lt 0) {
        throw "Opening brace not found for non-Windows system_ram_limit_gb."
    }

    $secondCloseBrace = Find-MatchingBrace -Text $Text -OpenBraceIndex $secondOpenBrace
    return $Text.Remove($windowsCfg.Index, $secondCloseBrace - $windowsCfg.Index + 1).Insert($windowsCfg.Index, $Replacement.TrimEnd())
}

$linuxRamFunction = @'
fn system_ram_limit_gb() -> u32 {
    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            let Some(rest) = line.strip_prefix("MemTotal:") else {
                continue;
            };

            let Some(kib) = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };

            let gib = 1024_u64 * 1024;
            return ((kib + gib - 1) / gib).clamp(1, u32::MAX as u64) as u32;
        }
    }

    32
}
'@

$linuxJavaFunction = @'
fn system_java_path() -> Option<PathBuf> {
        if let Some(java_path) = find_executable_on_path("java") {
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

        if let Ok(java_home) = env::var("JAVA_HOME") {
            let java_home_path = PathBuf::from(java_home).join("bin").join("java");
            if java_home_path.exists() {
                return Some(java_home_path);
            }
        }

        for base in ["/usr/lib/jvm", "/usr/java", "/opt/java"] {
            let Ok(entries) = fs::read_dir(base) else {
                continue;
            };

            for entry in entries.flatten() {
                let java_path = entry.path().join("bin").join("java");
                if java_path.exists() {
                    return Some(java_path);
                }
            }
        }

        None
    }
'@

$linuxInstallJavaFunction = @'
fn install_java_runtime(&self, ctx: &egui::Context) {
        if *self.is_installing_java.lock().unwrap() {
            return;
        }

        *self.is_installing_java.lock().unwrap() = true;
        *self.java_status.lock().unwrap() = "Installing Java 21 runtime...".to_string();
        *self.status_text.lock().unwrap() = "Installing Java runtime...".to_string();
        *self.java_status.lock().unwrap() =
            "Install OpenJDK 21 with your distro package manager, or rely on launcher-managed Java during launch.".to_string();
        *self.status_text.lock().unwrap() =
            "Linux Java install is manual; launch can still use managed Java.".to_string();
        self.log_to_terminal(
            "Linux port does not run privileged package-manager commands. Install OpenJDK 21 with your distro package manager if system Java is needed.",
        );
        *self.is_installing_java.lock().unwrap() = false;
        ctx.request_repaint();
    }
'@

$openFolderFunction = @'
fn open_folder(path: PathBuf) {
    let _ = Command::new("xdg-open").arg(path).spawn();
}

'@

$content = Replace-Ram-Function-Block -Text $content -Replacement $linuxRamFunction
$content = Replace-Function -Text $content -FunctionName "system_java_path" -Replacement $linuxJavaFunction
$content = Replace-Function -Text $content -FunctionName "install_java_runtime" -Replacement $linuxInstallJavaFunction

$content = $content -replace '\s*set_windows_app_user_model_id\(\);\r?\n', ''
$content = [regex]::Replace(
    $content,
    '(?s)\r?\n#\[cfg\(windows\)\]\s*fn set_windows_app_user_model_id\(\).*?#\[cfg\(not\(windows\)\)\]\s*fn set_windows_app_user_model_id\(\)\s*\{\}\r?\n',
    "`r`n"
)

if ($content -notmatch '(?m)^fn open_folder\(') {
    $content = $content -replace '(?m)^fn prepare_isolated_instance\(', ($openFolderFunction + 'fn prepare_isolated_instance(')
}

$pathHelper = @'
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

'@

if ($content -notmatch '(?m)^fn find_executable_on_path\(') {
    $content = $content -replace '(?m)^fn mod_metadata_matches_environment\(', ($pathHelper + 'fn mod_metadata_matches_environment(')
}

$content = $content -replace 'let _ = std::process::Command::new\("explorer"\)\.arg\(([^)]+)\)\.spawn\(\);', 'open_folder($1);'
$content = [regex]::Replace(
    $content,
    'let _ = std::process::Command::new\("explorer"\)\s*\.arg\(([^)]+)\)\s*\.spawn\(\);',
    'open_folder($1);'
)

Set-Content -LiteralPath $targetPath -Value $content -NoNewline

Push-Location (Join-Path $repoRoot "LINUX")
try {
    cargo fmt
} finally {
    Pop-Location
}

Write-Host "Synced src/main.rs into LINUX/src/main.rs and reapplied Linux-specific patches."
