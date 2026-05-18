# Launcher Scripts

Run the `.bat` files from File Explorer or from the repo root in Command Prompt/PowerShell. The `.bat` wrappers exist because double-clicking `.ps1` files often opens them in Notepad instead of running them.

## Push To GitHub

First-time setup when no `origin` remote exists:

Double-click or run this and paste your SSH remote when prompted:

```powershell
scripts\update-github.bat
```

Or pass the SSH remote directly:

```powershell
scripts\update-github.bat -RemoteUrl git@github.com:<user>/<repo>.git -Commit -Message "Update launcher" -SetUpstream
```

After `origin` is configured:

```powershell
scripts\update-github.bat -Commit -Message "Update launcher"
```

Use without `-Commit` if you already committed manually:

```powershell
scripts\update-github.bat -SetUpstream
```

## Sync Main Rust File To Linux Port

```powershell
scripts\sync-linux-main.bat
```

This copies `src/main.rs` to `LINUX/src/main.rs`, then reapplies the Linux-specific patches such as `xdg-open`, Linux Java lookup, Linux RAM detection, and removing Windows-only app setup.
