use std::path::Path;

const FORBIDDEN_COMPONENTS: &[&str] = &[
    ".ds_store",
    ".git",
    ".tmp",
    ".venv",
    "__pycache__",
    "build",
    "cache",
    "caches",
    "cachestorage",
    "code cache",
    "dist",
    "gpucache",
    "local storage",
    "logs",
    "node_modules",
    "process_manager",
    "session storage",
    "target",
    "tmp",
    "vendor_imports",
    "venv",
];

const FORBIDDEN_NAMES: &[&str] = &[
    "auth.json",
    "cookies",
    "cookies-journal",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "id_rsa",
    "login data",
    "login data for account",
    "login data for account-journal",
    "login data-journal",
    "runningchromeversion",
    "singletoncookie",
    "singletonlock",
    "singletonsocket",
];

pub fn is_forbidden(path: &Path) -> bool {
    let rendered = path.as_os_str().to_string_lossy().replace('\\', "/");

    rendered
        .split('/')
        .filter(|part| !part.is_empty())
        .any(|part| {
            let name = part.to_ascii_lowercase();
            FORBIDDEN_COMPONENTS.contains(&name.as_str())
                || FORBIDDEN_NAMES.contains(&name.as_str())
                || name == ".env"
                || name.starts_with(".env.")
                || (name.starts_with("logs_") && name.contains(".sqlite"))
                || [".ipc", ".key", ".pem", ".sock", ".socket"]
                    .iter()
                    .any(|extension| name.ends_with(extension))
        })
}
