# Fusion Launcher Linux Port

This folder is a standalone Linux-oriented copy of the launcher.

## Layout

The launcher keeps isolated Minecraft instances under:

```text
instances/<mc_version>-<loader>/
  mods/
  config/
  saves/
  resourcepacks/
  shaderpacks/
  logs/
```

Minecraft launches with `--gameDir` pointed at the active instance, so loaders only see `<gameDir>/mods`.

## Build

```bash
cargo build --release
```

## Notes

- Folder-opening actions use `xdg-open` on Linux.
- Java detection checks `java` on `PATH`, `JAVA_HOME/bin/java`, and common Linux JVM directories.
- The in-app Java install button does not run privileged package-manager commands on Linux. Install OpenJDK 21 with your distro package manager if you want system Java; launching can still use the launch backend's managed Java path.
