use crate::backup::{run_backup_with_app, Status};
use crate::config::{
    ensure_agent_defaults, resolve_hostname, AppConfig, Destination, DestinationKind,
    LocalDestination,
};
use crate::presets::load_presets;
use std::fs;

#[test]
fn presets_parse_and_backup_local() {
    let config_dir = std::env::temp_dir().join("agent-backup-smoke-config");
    let _ = fs::remove_dir_all(&config_dir);
    std::env::set_var("AGENT_BACKUP_CONFIG_DIR", &config_dir);

    let loaded = load_presets().expect("presets");
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert!(loaded.agents.iter().any(|a| a.key == "cursor"));
    assert!(loaded.agents.iter().any(|a| a.key == "cursor-cli"));

    let mut cfg = AppConfig::default();
    ensure_agent_defaults(&mut cfg, &loaded.presets);
    cfg.encryption.enabled = false;

    let dest = std::env::temp_dir().join("agent-backup-smoke-dest");
    let _ = fs::remove_dir_all(&dest);
    fs::create_dir_all(&dest).unwrap();
    cfg.destinations.push(Destination {
        id: "dest_test".into(),
        name: "tmp".into(),
        enabled: true,
        kind: DestinationKind::Local {
            local: LocalDestination {
                root_path: dest.to_string_lossy().into(),
            },
        },
    });

    let result = run_backup_with_app(None, &cfg, "manual").expect("backup");
    assert!(
        matches!(result.overall_status, Status::Ok | Status::Partial),
        "{}",
        result.message
    );

    let host = resolve_hostname(&cfg);
    let dir = dest.join("backups").join(host).join(&result.created_at);
    assert!(
        dir.join("manifest.json").exists(),
        "missing manifest in {:?}",
        dir
    );
}
