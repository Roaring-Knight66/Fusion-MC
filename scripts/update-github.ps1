param(
    [string]$RemoteUrl = "",
    [string]$Branch = "",
    [string]$Message = "",
    [switch]$Commit,
    [switch]$SetUpstream
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    throw "git was not found on PATH."
}

$insideRepo = git rev-parse --is-inside-work-tree 2>$null
if ($insideRepo -ne "true") {
    throw "This script must be run from inside a git repository."
}

if ([string]::IsNullOrWhiteSpace($Branch)) {
    $Branch = git branch --show-current
}
if ([string]::IsNullOrWhiteSpace($Branch)) {
    throw "Could not determine the current branch. Pass -Branch <name>."
}

$remotes = @(git remote)
$hasOrigin = $remotes -contains "origin"
$originUrl = ""
if ($hasOrigin) {
    $originUrl = git remote get-url origin
}

if (-not $hasOrigin -or [string]::IsNullOrWhiteSpace($originUrl)) {
    if ([string]::IsNullOrWhiteSpace($RemoteUrl)) {
        Write-Host ""
        Write-Host "No origin remote is configured."
        $RemoteUrl = Read-Host "Paste your GitHub SSH remote, like git@github.com:USER/REPO.git"
        if ([string]::IsNullOrWhiteSpace($RemoteUrl)) {
            Write-Host "No remote URL entered. Nothing was pushed."
            exit 1
        }
    }

    git remote add origin $RemoteUrl
    $originUrl = $RemoteUrl
    Write-Host "Added origin remote: $originUrl"
} elseif (-not [string]::IsNullOrWhiteSpace($RemoteUrl) -and $RemoteUrl -ne $originUrl) {
    git remote set-url origin $RemoteUrl
    $originUrl = $RemoteUrl
    Write-Host "Updated origin remote: $originUrl"
}

git status --short

if (-not $Commit) {
    $commitChoice = Read-Host "Commit all current changes before pushing? [Y/n]"
    if ([string]::IsNullOrWhiteSpace($commitChoice) -or $commitChoice.Trim().ToLowerInvariant().StartsWith("y")) {
        $Commit = $true
    }
}

if ($Commit) {
    if ([string]::IsNullOrWhiteSpace($Message)) {
        $enteredMessage = Read-Host "Commit message [Update launcher]"
        if ([string]::IsNullOrWhiteSpace($enteredMessage)) {
            $Message = "Update launcher"
        } else {
            $Message = $enteredMessage
        }
    }

    git add -A
    $staged = git diff --cached --name-only
    if (-not [string]::IsNullOrWhiteSpace($staged)) {
        git commit -m $Message
    } else {
        Write-Host "No staged changes to commit."
    }
}

git push -u origin $Branch

Write-Host "Pushed $Branch to origin."
