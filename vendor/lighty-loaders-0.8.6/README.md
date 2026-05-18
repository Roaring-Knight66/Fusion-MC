# lighty-loaders

Minecraft mod loader support for [LightyLauncher](https://crates.io/crates/lighty-launcher).

## Note

This is an internal crate for the LightyLauncher ecosystem. Most users should use the main [`lighty-launcher`](https://crates.io/crates/lighty-launcher) crate instead.

## Features

- **Multiple Loaders**: Vanilla, Fabric, Quilt, Forge, NeoForge, OptiFine, LightyUpdater
- **Smart Caching**: Dual cache system with configurable TTL
- **Version Management**: Query and resolve loader versions
- **Metadata Merging**: Combine multiple loader metadata
- **Feature Flags**: Compile only the loaders you need

## Structure

```
lighty-loaders/
└── src/
    ├── lib.rs                  # Module declarations and re-exports
    ├── loaders/                # Loader implementations
    │   ├── mod.rs
    │   ├── vanilla/            # Vanilla Minecraft
    │   │   ├── vanilla.rs
    │   │   └── vanilla_metadata.rs
    │   ├── fabric/             # Fabric loader
    │   │   ├── fabric.rs
    │   │   └── fabric_metadata.rs
    │   ├── quilt/              # Quilt loader
    │   │   ├── quilt.rs
    │   │   └── quilt_metadata.rs
    │   ├── forge/              # Forge loader
    │   │   ├── forge.rs
    │   │   ├── forge_legacy.rs
    │   │   └── forge_metadata.rs
    │   ├── neoforge/           # NeoForge loader
    │   │   ├── neoforge.rs
    │   │   └── neoforge_metadata.rs
    │   ├── optifine/           # OptiFine
    │   │   ├── optifine.rs
    │   │   └── optifine_metadata.rs
    │   └── lighty_updater/     # Custom updater system
    │       ├── lighty_updater.rs
    │       ├── lighty_metadata.rs
    │       └── merge_metadata.rs
    ├── types/                  # Common types
    │   ├── mod.rs              # Type declarations
    │   └── version_metadata.rs # Version metadata structures
    ├── utils/                  # Utilities
    │   ├── mod.rs
    │   ├── cache.rs            # Dual cache system
    │   ├── error.rs            # Error types
    │   ├── manifest.rs         # Manifest repository
    │   └── query.rs            # Query trait
    └── lib.rs                  # Main exports
```

## Usage

```toml
[dependencies]
lighty-loaders = { version = "0.6.3", features = ["all-loaders"] }
```

```rust
use lighty_loaders::version::{Version, Loader};
use directories::ProjectDirs;

#[tokio::main]
async fn main() {
    let launcher_dir = ProjectDirs::from("com", "MyLauncher", "").unwrap();

    // Create a Fabric instance
    let mut version = Version::new(
        "fabric-1.21",
        Loader::Fabric,
        "0.16.9",      // Fabric loader version
        "1.21",        // Minecraft version
        &launcher_dir
    );

    // Launch the game
    version.launch("Player", "uuid", JavaDistribution::Temurin).await?;
}
```

## Supported Loaders

### Vanilla

Pure Minecraft without modifications.

```rust
use lighty_loaders::{Loader, Version};

let version = Version::new("vanilla-1.21", Loader::Vanilla, "", "1.21", &dir);
```

**Status**: Stable

### Fabric

Lightweight mod loader with excellent performance.

```rust
let version = Version::new("fabric-1.21", Loader::Fabric, "0.16.9", "1.21", &dir);
```

**Status**: Stable
**Example Versions**: 0.15.11, 0.16.0, 0.16.9

### Quilt

Fork of Fabric with additional features.

```rust
let version = Version::new("quilt-1.21", Loader::Quilt, "0.27.1", "1.21", &dir);
```

**Status**: Stable
**Example Versions**: 0.26.0, 0.27.0, 0.27.1

### Forge

Traditional mod loader with extensive mod support.

```rust
let version = Version::new("forge-1.21", Loader::Forge, "51.0.38", "1.21", &dir);
```

**Status**: Testing
**Example Versions**: 47.3.0, 50.1.0, 51.0.38

### NeoForge

Modern fork of Forge for newer Minecraft versions.

```rust
let version = Version::new("neoforge-1.21", Loader::NeoForge, "21.1.80", "1.21", &dir);
```

**Status**: Testing
**Example Versions**: 20.4.109, 21.0.167, 21.1.80

### OptiFine

Performance and graphics optimization mod.

```rust
let version = Version::new("optifine-1.21", Loader::Optifine, "HD_U_I9", "1.21", &dir);
```

**Status**: Experimental
**Example Versions**: HD_U_I8, HD_U_I9

## Features Flags

Control which loaders are compiled:

```toml
# All loaders
lighty-loaders = { version = "0.6.3", features = ["all-loaders"] }

# Specific loaders
lighty-loaders = { version = "0.6.3", features = ["vanilla", "fabric", "quilt"] }
```

Available features:
- `vanilla` - Vanilla Minecraft
- `fabric` - Fabric loader
- `quilt` - Quilt loader
- `neoforge` - NeoForge loader
- `forge` - Forge loader
- `forge_legacy` - Legacy Forge (1.7.10-1.12.2)
- `lighty_updater` - Custom updater system
- `all-loaders` - Enable all loaders

## Caching System

The loader uses a dual cache architecture:

1. **Raw Version Cache**: Stores complete JSON manifests
2. **Query Cache**: Stores extracted data by query

Each cache features:
- Configurable TTL per data type
- Automatic cleanup
- Thread-safe with `Arc<RwLock<HashMap>>`

## License

MIT

## Links

- **Main Package**: [lighty-launcher](https://crates.io/crates/lighty-launcher)
- **Repository**: [GitHub](https://github.com/Lighty-Launcher/LightyLauncherLib)
- **Documentation**: [docs.rs/lighty-loaders](https://docs.rs/lighty-loaders)
