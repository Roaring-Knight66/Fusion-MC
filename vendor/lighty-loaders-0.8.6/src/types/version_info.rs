use std::path::Path;

/// Trait générique pour représenter les informations d'une version
///
/// Ce trait est utilisé par `ManifestRepository` pour supporter différents types de versions
/// (Version standard, LightyVersion, etc.)
///
/// # Exemples
///
/// ```rust
/// use lighty_loaders::types::VersionInfo;
/// use lighty_version::VersionBuilder;
/// use lighty_loaders::types::Loader;
///
/// fn print_version_info<V: VersionInfo>(version: &V) {
///     println!("Name: {}", version.name());
///     println!("Minecraft: {}", version.minecraft_version());
///     println!("Game dir: {}", version.game_dirs().display());
/// }
/// ```
pub trait VersionInfo: Clone + Send + Sync {
    /// Type du loader utilisé (Vanilla, Fabric, NeoForge, etc.)
    type LoaderType: Clone + Send + Sync + std::fmt::Debug;

    /// Nom de la version (identifiant unique du profil)
    fn name(&self) -> &str;

    /// Version du loader (ou URL du serveur pour LightyVersion)
    ///
    /// Exemples : "0.15.0" pour Fabric, "21.0.0" pour NeoForge
    fn loader_version(&self) -> &str;

    /// Version de Minecraft
    ///
    /// Exemples : "1.20.1", "1.19.4"
    fn minecraft_version(&self) -> &str;

    /// Répertoire de jeu (contient les assets, libraries, versions, etc.)
    fn game_dirs(&self) -> &Path;

    /// Répertoire Java (contient les installations JRE)
    fn java_dirs(&self) -> &Path;

    /// Retourne le loader utilisé
    fn loader(&self) -> &Self::LoaderType;

    // === Méthodes utilitaires avec implémentations par défaut ===

    /// Vérifie si le répertoire de jeu existe
    fn game_dir_exists(&self) -> bool {
        self.game_dirs().exists()
    }

    /// Vérifie si le répertoire Java existe
    fn java_dir_exists(&self) -> bool {
        self.java_dirs().exists()
    }

    /// Retourne un identifiant complet de la version
    ///
    /// Format : `{name}-{minecraft_version}-{loader_version}`
    fn full_identifier(&self) -> String {
        format!(
            "{}-{}-{}",
            self.name(),
            self.minecraft_version(),
            self.loader_version()
        )
    }

    /// Retourne les chemins importants sous forme de tuple
    ///
    /// Utile pour le logging ou le debugging
    fn paths(&self) -> (&Path, &Path) {
        (self.game_dirs(), self.java_dirs())
    }
}
